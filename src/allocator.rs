// src/allocator.rs

pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }
    // Fixed the lifetime warning here
    pub fn lock(&self) -> spin::MutexGuard<'_, A> {
        self.inner.lock()
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
            slab:crate::slab::SlabAllocator::new(),
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

#[global_allocator]
pub static ALLOCATOR: Locked<HeapAllocator> = Locked::new(HeapAllocator::new());

unsafe impl core::alloc::GlobalAlloc for Locked<HeapAllocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let mut heap = self.lock();
        let size = layout.size();

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
            // We bypass the virtual bump allocator and directly map contiguous physical memory.
            let order = crate::buddy::size_to_order(size);

            let mut buddy = crate::memory::FRAME_ALLOCATOR.lock();
            match buddy.alloc(order) {
                Some(ptr) => ptr as *mut u8,
                None => core::ptr::null_mut(),
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let mut heap = self.lock();
        let size = layout.size();

        if crate::slab::SlabAllocator::list_index(size).is_some() {
            // Traffic Cop: Send small blocks to the Slab
            heap.slab.free(ptr, size);
        } else {
            // Traffic Cop: Bypass Slab, send massive blocks directly to Buddy
            let order = crate::buddy::size_to_order(size);
            
            crate::memory::FRAME_ALLOCATOR.lock().free(ptr as usize, order);
        }
    }
}
unsafe impl Send for HeapAllocator {}
unsafe impl Sync for HeapAllocator {}
