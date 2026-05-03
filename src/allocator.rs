// src/allocator.rs

pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

pub struct InterruptSafeGuard<'a, A> {
    // Wrapped in Option so we can manually drop the lock first
    inner: Option<spin::MutexGuard<'a, A>>,
    interrupts_were_enabled: bool,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }
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
    fn drop(&mut self) {
        //  Explicitly drop the spinlock FIRST
        self.inner.take();

        // NOW it is safe to restore the hardware interrupt state
        if self.interrupts_were_enabled {
            x86_64::instructions::interrupts::enable();
        }
    }
}

pub struct HeapAllocator {
    slab: crate::slab::SlabAllocator,
    virtual_bump_ptr: usize,
    heap_end: usize,
}

impl HeapAllocator {
    pub const fn new() -> Self {
        HeapAllocator {
            slab: crate::slab::SlabAllocator::new(),
            virtual_bump_ptr: 0,
            heap_end: 0,
        }
    }

    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        self.virtual_bump_ptr = heap_start;
        self.heap_end = heap_start + heap_size;
    }

    pub fn allocate_virtual_page(&mut self) -> Option<*mut u8> {
        if self.virtual_bump_ptr + 4096 > self.heap_end {
            return None; // Out of virtual address space!
        }

        let page_addr = self.virtual_bump_ptr;
        self.virtual_bump_ptr += 4096;

        Some(page_addr as *mut u8)
    }
}

pub fn align_to(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

#[global_allocator]
pub static ALLOCATOR: Locked<HeapAllocator> = Locked::new(HeapAllocator::new());

unsafe impl core::alloc::GlobalAlloc for Locked<HeapAllocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let mut heap = self.lock();
        let size = align_to(layout.size(), layout.align());

        // FAST PATH & CACHE MISS: Handled by the Slab Allocator
        if let Some(index) = crate::slab::SlabAllocator::list_index(size) {
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

            let order = crate::buddy::size_to_order(page_aligned_size);

            let mut buddy = crate::memory::FRAME_ALLOCATOR.lock();
            match buddy.alloc(order) {
                Some(ptr) => ptr as *mut u8,
                None => core::ptr::null_mut(),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let mut heap = self.lock();
        let size = align_to(layout.size(), layout.align());

        if crate::slab::SlabAllocator::list_index(size).is_some() {
            // Traffic Cop: Send small blocks to the Slab
            heap.slab.free(ptr, size);
        } else {
            // Traffic Cop: Bypass Slab, send massive blocks directly to Buddy
            let page_aligned_size = align_to(size, 4096);
            let order = crate::buddy::size_to_order(page_aligned_size);

            crate::memory::FRAME_ALLOCATOR
                .lock()
                .free(ptr as usize, order);
        }
    }
}
unsafe impl Send for HeapAllocator {}
unsafe impl Sync for HeapAllocator {}
