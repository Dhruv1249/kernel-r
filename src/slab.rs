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
    pub fn new() -> Self {
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

    pub fn alloc(&mut self, buddy: &mut crate::buddy::BuddyAllocator, size: usize) -> *mut u8 {
        match Self::list_index(size) {
            None => {
                // FALLBACK: Size is too big for the slab.
                let order = crate::buddy::size_to_order(size);
                let block = buddy.alloc(order).expect("OOM: Buddy out of memory!");
                return block as *mut u8;
            }
            Some(index) => {
                let head = self.list_heads[index];

                if !head.is_null() {
                    // FAST PATH: We have a free block!
                    self.list_heads[index] = unsafe { (*head).next };
                    return head as *mut u8;
                } else {
                    // CACHE MISS: The list is empty. Time to carve a new page.
                    let page_ptr = buddy.alloc(0).expect("OOM: Buddy out of memory!") as *mut u8;
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
                    
                    return page_ptr;
                }
            }
        }
    }
}
