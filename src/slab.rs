// src/slab.rs

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

struct ListNode {
    next: *mut ListNode,
}

pub struct SlabAllocator {
    // Array of list heads
    list_heads: [*mut ListNode; BLOCK_SIZES.len()],
}

impl SlabAllocator {
    pub const fn new() -> Self {
        SlabAllocator {
            list_heads: [core::ptr::null_mut(); BLOCK_SIZES.len()],
        }
    }

    pub fn list_index(size: usize) -> Option<usize> {
        if size > BLOCK_SIZES[BLOCK_SIZES.len() - 1] {
            return None;
        }
        for i in 0..BLOCK_SIZES.len() {
            if size <= BLOCK_SIZES[i] {
                return Some(i);
            }
        }
        None
    }

    pub fn alloc(&mut self, size: usize) -> Option<*mut u8> {
        match Self::list_index(size) {
            None => {
                // FALLBACK: Size is too big for the slab.
                return None;
            }
            Some(index) => {
                let head = self.list_heads[index];

                if !head.is_null() {
                    // FAST PATH: We have a free block!
                    self.list_heads[index] = unsafe { (*head).next };
                    return Some(head as *mut u8);
                } else {
                    // CACHE MISS
                    return None;
                }
            }
        }
    }

    pub unsafe fn populate_cache(&mut self, index: usize, page_ptr: *mut u8) {
        let block_size = BLOCK_SIZES[index];
        let num_blocks = 4096 / block_size;

        // Loop through and link: block 0 -> block 1 -> block 2...
        let mut current_ptr = page_ptr;

        for _ in 0..(num_blocks - 1) {
            let next_ptr = unsafe { current_ptr.add(block_size) };
            let node = current_ptr as *mut ListNode;

            unsafe {
                (*node).next = next_ptr as *mut ListNode;
            }

            current_ptr = next_ptr;
        }

        // The last block must point to null to terminate the list!
        unsafe {
            (*(current_ptr as *mut ListNode)).next = core::ptr::null_mut();
        }

        // We return the very first block (page_ptr) to the user.
        // The rest of the list (starting at block 1) becomes our new free list!
        self.list_heads[index] = unsafe { page_ptr.add(block_size) as *mut ListNode };
    }

    pub fn free(&mut self, ptr: *mut u8, size: usize) {
        match Self::list_index(size) {
            None => {
                // This is a safety net. The HeapAllocator should have caught this!
                panic!(
                    "FATAL: SlabAllocator asked to free oversized block ({} bytes)",
                    size
                );
            }
            Some(index) => {
                let head = self.list_heads[index];
                let current_ptr = ptr as *mut ListNode;
                unsafe {
                    (*current_ptr).next = head;
                    self.list_heads[index] = current_ptr;
                }
            }
        }
    }
}
