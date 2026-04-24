// src/allocator.rs

use core::usize;

pub struct ListNode {
    size: usize,
    next: Option<&'static mut ListNode>,
}

pub struct LinkedListAllocator {
    head: ListNode,
}

impl LinkedListAllocator {
    pub const fn new() -> Self {
        let head = ListNode {
            size: 0,
            next: None,
        };
        LinkedListAllocator { head }
    }

    // We mark this function unsafe because the caller must guarantee
    // that the given memory range is valid, mapped, and not used by anything else!
    pub unsafe fn init(&mut self, heap_start: usize, heap_size: usize) {
        let heap_pointer = heap_start as *mut ListNode;
        unsafe {
            (*heap_pointer).size = heap_size;
            (*heap_pointer).next = None;
        }
        // Convert the raw pointer into a static mutable reference.
        // The compiler infers the &'static mut lifetime because we are
        // assigning it to `next`, which requires a static lifetime.
        let first_free_block: &'static mut ListNode;
        unsafe {
            first_free_block = &mut *heap_pointer;
        }
        // Link the dummy head to our new giant free block
        self.head.next = Some(first_free_block);
    }

    pub fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }

    pub fn align_down(addr: usize, align: usize) -> usize {
        addr & !(align - 1)
    }

    pub fn alloc_from_region(
        region_start: usize,
        region_size: usize,
        size: usize,
        align: usize,
    ) -> Option<usize> {
        let alloc_start = Self::align_up(region_start, align);
        let alloc_end = alloc_start.checked_add(size)?;

        let region_end = region_start + region_size;

        // Does it fit in the region at all?
        if alloc_end > region_end {
            return None;
        }

        // Is the leftover space valid?
        let leftover_space = region_end - alloc_end;
        if leftover_space > 0 && leftover_space < core::mem::size_of::<ListNode>() {
            return None; // It fits, but the scrap leftover is too small to track!
        }

        // It fits perfectly, and the leftover is large enough.
        Some(alloc_start)
    }

    pub fn size_align(layout: core::alloc::Layout) -> (usize, usize) {
        let layout = layout
            .align_to(core::mem::align_of::<ListNode>())
            .expect("Alignment error")
            .pad_to_align();
        let size = layout.size().max(core::mem::size_of::<ListNode>());
        (size, layout.align())
    }

    pub fn alloc(&mut self, layout: core::alloc::Layout) -> Option<*mut u8> {
        // Adjust size and alignment
        let (size, align) = Self::size_align(layout);

        // Start at dummy head
        let mut current = &mut self.head;

        // Traverse the list
        while let Some(region) = current.next.take() {
            if let Some(alloc_start) =
                Self::alloc_from_region(region as *const _ as usize, region.size, size, align)
            {
                // We found a block

                let next_region = region.next.take();

                // Address of this allocation
                let alloc_end = alloc_start.checked_add(size)?;

                // Calculate the leftover space
                let region_end = region as *const _ as usize + region.size;
                let leftover_space = region_end - alloc_end;

                if leftover_space > 0 {
                    let list = alloc_end as *mut ListNode;
                    unsafe {
                        (*list).size = leftover_space;
                        (*list).next = next_region;
                        current.next = Some(&mut *list);
                    }
                } else {
                    // Perfect fit
                    current.next = next_region;
                }

                // Return the address of the allocated block
                return Some(alloc_start as *mut u8);
            }
            // It didn't fit. Put the region back into the list exactly where we found it!
            current.next = Some(region);
            // Move `current` forward to the node we just put back
            current = current.next.as_mut().unwrap();
        }

        None
    }

    pub fn dealloc(&mut self, ptr: *mut u8, layout: core::alloc::Layout) {
        let head_addr = &self.head as *const ListNode as usize;
        let (size, _) = LinkedListAllocator::size_align(layout);
        let free_ptr = ptr as usize;
        let mut current = &mut self.head;

        while let Some(region) = current.next.take() {
            let next_start = region as *const _ as usize;
            // If the next region starts after the free pointer, we are done
            if next_start > free_ptr as usize {
                current.next = Some(region);
                break;
            }
            current.next = Some(region);
            current = current.next.as_mut().unwrap();
        }

        let current_addr = current as *const _ as usize;

        let is_head = current as *const _ as usize == head_addr;
        let merges_left = !is_head && (current_addr + current.size == free_ptr);

        let merges_right = if let Some(ref mut next_region) = current.next {
            free_ptr + size == (*next_region) as *const _ as usize
        } else {
            false
        };

        if merges_left && merges_right {
            // Merge the two regions
            let next_region = current.next.take().unwrap();
            current.size += size + next_region.size;
            current.next = next_region.next.take();
        } else if merges_left {
            // Merge the left region
            current.size += size;
        } else if merges_right {
            // Merge the right region
            let next_region = current.next.take().unwrap();
            let new_ptr = free_ptr as *mut ListNode;
            unsafe {
                (*new_ptr).size = size + next_region.size;
                (*new_ptr).next = next_region.next.take();
            }
            unsafe {
                current.next = Some(&mut *new_ptr);
            }
        } else {
            let new_ptr = free_ptr as *mut ListNode;
            unsafe {
                (*new_ptr).size = size;
                (*new_ptr).next = current.next.take();
                current.next = Some(&mut *new_ptr);
            }
        }
    }
}

// Thread safe wrapper for allocator

pub struct Locked<A> {
    inner: spin::Mutex<A>,
}

impl<A> Locked<A> {
    pub const fn new(inner: A) -> Self {
        Locked {
            inner: spin::Mutex::new(inner),
        }
    }
    pub fn lock(&self) -> spin::MutexGuard<A> {
        self.inner.lock()
    }
}

unsafe impl core::alloc::GlobalAlloc for Locked<LinkedListAllocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let mut allocator = self.lock();

        // Global allocator requires null pointer on format not Option
        match allocator.alloc(layout) {
            Some(ptr) => ptr as *mut u8,
            None => ::core::ptr::null_mut(),
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        let mut allocator = self.lock();
        allocator.dealloc(ptr, layout);
    }
}
