// src/mm/allocator.rs

/// A spinlock wrapper that automatically disables hardware interrupts while locked.
///
/// `Locked<A>` wraps any type `A` in a `spin::Mutex` and provides a single
/// `lock()` method that:
/// 1. Saves the current interrupt-enable state (RFLAGS.IF).
/// 2. Disables interrupts if they were enabled, preventing a timer ISR from
///    running and trying to acquire the same lock — which would deadlock.
/// 3. Acquires the spinlock.
/// 4. Returns an [`InterruptSafeGuard`] that re-enables interrupts when dropped.
///
/// This pattern is essential for data structures shared between normal kernel
/// code and interrupt handlers (e.g. the global frame allocator, the VGA
/// writer, the serial port).
pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

/// RAII guard returned by [`Locked::lock`].
///
/// Derefs transparently to `A`, giving callers direct access to the protected
/// data.  On drop:
/// 1. Explicitly drops the inner `spin::Mutex` guard, releasing the spinlock.
/// 2. Restores the hardware interrupt state that was saved before locking.
///
/// The two-step drop order is critical: the interrupt state must only be
/// restored **after** the lock is released, otherwise another CPU (or re-entrant
/// interrupt) could acquire the lock while we are still running inside the
/// critical section.
pub struct InterruptSafeGuard<'a, A> {
    // Wrapped in Option so we can manually drop the lock first
    inner: Option<spin::MutexGuard<'a, A>>,
    interrupts_were_enabled: bool,
}

impl<A> Locked<A> {
    /// Creates a new `Locked<A>` wrapping `inner` in a spinlock.
    ///
    /// This is a `const` function, allowing `Locked<A>` to be used in
    /// `static` initialisers without requiring runtime initialisation.
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }

    /// Acquires the lock, disabling interrupts for the duration of the critical section.
    ///
    /// Saves the current interrupt enable state, disables interrupts, then
    /// spins until the spinlock is acquired.  Returns an [`InterruptSafeGuard`]
    /// that releases both the lock and restores the interrupt state on drop.
    pub fn lock(&self) -> InterruptSafeGuard<'_, A> {
        let saved_state = x86_64::instructions::interrupts::are_enabled();

        if saved_state {
            x86_64::instructions::interrupts::disable();
        }

        InterruptSafeGuard {
            inner: Some(self.inner.lock()),
            interrupts_were_enabled: saved_state,
        }
    }
}

// Implement Deref so we can use the guard transparently
impl<'a, A> core::ops::Deref for InterruptSafeGuard<'a, A> {
    type Target = A;
    fn deref(&self) -> &A {
        self.inner.as_ref().expect("Guard used after drop")
    }
}

impl<'a, A> core::ops::DerefMut for InterruptSafeGuard<'a, A> {
    fn deref_mut(&mut self) -> &mut A {
        self.inner.as_mut().expect("Guard used after drop")
    }
}

impl<'a, A> Drop for InterruptSafeGuard<'a, A> {
    /// Releases the spinlock first, then restores the saved interrupt state.
    ///
    /// The explicit `take()` drops the `MutexGuard` (and thus spins the lock
    /// open) before the `if` that re-enables interrupts.  This ordering
    /// prevents the window where interrupts are enabled but the lock is still
    /// held.
    fn drop(&mut self) {
        //  Explicitly drop the spinlock FIRST
        self.inner.take();

        // NOW it is safe to restore the hardware interrupt state
        if self.interrupts_were_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}

/// The kernel's global heap allocator, combining slab and buddy strategies.
///
/// # Architecture
///
/// `HeapAllocator` implements a two-tier allocation strategy:
///
/// **Tier 1 — Slab (≤ 2048 bytes):**  
/// Fixed-size slab caches give O(1) allocation and deallocation with zero
/// fragmentation for common small objects (Box, Vec elements, etc.).
///
/// **Tier 2 — Buddy (> 2048 bytes):**  
/// Large allocations are rounded up to the nearest power-of-two page count
/// and served directly from the buddy frame allocator.
///
/// A virtual bump pointer (`virtual_bump_ptr`) tracks the next available
/// address in the kernel heap virtual-address region.  When the slab cache for
/// a given size class is empty, one 4 KiB page is carved out of this region
/// and handed to the slab to populate its cache.
pub struct HeapAllocator {
    slab: crate::mm::slab::SlabAllocator,
    virtual_bump_ptr: usize,
    heap_end: usize,
}

impl HeapAllocator {
    /// Creates an uninitialised `HeapAllocator` suitable for use as a `static`.
    ///
    /// All fields are zeroed; [`HeapAllocator::init`] must be called before any
    /// allocation attempt.
    pub const fn new() -> Self {
        HeapAllocator {
            slab: crate::mm::slab::SlabAllocator::new(),
            virtual_bump_ptr: 0,
            heap_end: 0,
        }
    }

    /// Configures the heap virtual-address window `[heap_start, heap_start + heap_size)`.
    ///
    /// Pages within this window are demand-mapped on first access via the page-fault
    /// handler (see `mm::paging` and `interrupts::interrupt`).  The bump pointer
    /// starts at `heap_start` and advances by 4 KiB each time the slab needs a
    /// fresh page.
    ///
    /// # Safety
    /// Must only be called once, after paging is active, with a virtual address range
    /// that is not already mapped.
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.virtual_bump_ptr = heap_start;
        self.heap_end = heap_start + heap_size;
    }

    /// Returns the next 4 KiB virtual page from the heap window, or `None` if exhausted.
    ///
    /// The function does **not** map the page — the page-fault handler performs
    /// demand mapping when the page is first written.  The bump pointer is advanced
    /// unconditionally after a successful check.
    pub fn allocate_virtual_page(&mut self) -> Option<*mut u8> {
        if self.virtual_bump_ptr + 4096 > self.heap_end {
            return None; // Out of virtual address space!
        }

        let page_addr = self.virtual_bump_ptr;
        self.virtual_bump_ptr += 4096;

        Some(page_addr as *mut u8)
    }
}

/// Rounds `addr` up to the next multiple of `align`.
///
/// `align` must be a power of two.  The trick `(addr + align - 1) & !(align - 1)`
/// sets the low `log2(align)` bits of `addr + align - 1` to zero, which is
/// equivalent to ceiling division by `align` then multiplication.
pub fn align_to(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

/// The kernel-wide global allocator instance, registered with `#[global_allocator]`.
///
/// All `alloc`/`dealloc` calls from Rust's `alloc` crate (Box, Vec, String, …)
/// are routed through this static via the `GlobalAlloc` trait implementation below.
/// It is protected by [`Locked`] to serialise concurrent access and prevent
/// interrupt-handler re-entrancy.
#[global_allocator]
pub static ALLOCATOR: Locked<HeapAllocator> = Locked::new(HeapAllocator::new());

unsafe impl core::alloc::GlobalAlloc for Locked<HeapAllocator> {
    /// Allocates memory according to `layout`.
    ///
    /// # Routing logic
    ///
    /// 1. Align `layout.size()` to `layout.align()`.
    /// 2. If the aligned size fits in a slab class (`<= 2048` bytes):
    ///    - **Cache hit**: pop from the slab free list — O(1).
    ///    - **Cache miss**: get a fresh virtual page, call `populate_cache`,
    ///      and return the first block from that page.
    /// 3. If the size exceeds 2048 bytes:
    ///    - Round up to the nearest 4 KiB page boundary.
    ///    - Reject allocations larger than 4 MiB (would exhaust the buddy tree).
    ///    - Call `buddy.alloc(order)` and return the raw pointer.
    ///
    /// Returns `null` on any failure (OOM).
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let mut heap = self.lock();
        let size = align_to(layout.size(), layout.align());

        // FAST PATH & CACHE MISS: Handled by the Slab Allocator
        if let Some(index) = crate::mm::slab::SlabAllocator::list_index(size) {
            //  Try to get a free block from the cache
            if let Some(ptr) = heap.slab.alloc(size) {
                return ptr;
            }

            //  Cache Miss: Allocate a virtual page and populate the cache
            if let Some(new_page) = heap.allocate_virtual_page() {
                unsafe {
                    heap.slab.populate_cache(index, new_page);
                }
                // populate_cache skips the first block so we can return it directly!
                return new_page;
            } else {
                return core::ptr::null_mut(); // OOM on virtual memory
            }
        } else {
            // FALLBACK: Massive allocation (> 2048 bytes).
            //  Align the requested size to the nearest 4KB (4096 bytes) page boundary.
            let page_aligned_size = align_to(size, 4096);

            // HARD LIMIT: Prevent allocations larger than 4MB (MAX_ORDER 10 = 1024 frames)
            if page_aligned_size > 4 * 1024 * 1024 {
                crate::serial_println!(
                    "WARNING: Kernel heap refused {} bytes (exceeds 4MB limit)",
                    page_aligned_size
                );
                return core::ptr::null_mut();
            }

            let order = crate::mm::buddy::size_to_order(page_aligned_size);

            let mut buddy = crate::mm::memory::FRAME_ALLOCATOR.lock();
            match buddy.alloc(order) {
                Some(ptr) => ptr as *mut u8,
                None => core::ptr::null_mut(),
            }
        }
    }

    /// Deallocates the memory at `ptr` that was previously returned by `alloc`.
    ///
    /// # Routing logic
    ///
    /// Small blocks (slab-eligible) are returned to the slab free list.
    /// Large blocks bypass the slab and go directly back to the buddy allocator,
    /// where coalescing with their buddy may occur.
    ///
    /// The routing decision is made purely from `layout.size()` — no per-block
    /// metadata is stored.
    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let mut heap = self.lock();
        let size = align_to(layout.size(), layout.align());

        if crate::mm::slab::SlabAllocator::list_index(size).is_some() {
            // Traffic Cop: Send small blocks to the Slab
            heap.slab.free(ptr, size);
        } else {
            // Traffic Cop: Bypass Slab, send massive blocks directly to Buddy
            let page_aligned_size = align_to(size, 4096);
            let order = crate::mm::buddy::size_to_order(page_aligned_size);

            crate::mm::memory::FRAME_ALLOCATOR
                .lock()
                .free(ptr as usize, order);
        }
    }
}
unsafe impl Send for HeapAllocator {}
unsafe impl Sync for HeapAllocator {}
