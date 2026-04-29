// src/buddy.rs

pub struct FreeBlock {
    next: Option<&'static mut FreeBlock>,
}

const MAX_ORDER: usize = 11; // Order 0 to 10

pub struct BuddyAllocator {
    // Array of 11 linked lists. Index 0 is 4KB, Index 1 is 8KB, etc. upto 4MB
    free_list: [Option<&'static mut FreeBlock>; MAX_ORDER],
}

pub fn size_to_order(size_in_bytes: usize) -> usize {
    // Add 4095 to ceiling the value, then bit-shift right by 12 to divide by 4096
    let pages = (size_in_bytes + 4095) >> 12;

    // The CPU natively calculates the next power of 2 and counts the zeros
    let order = pages.next_power_of_two().trailing_zeros() as usize;

    // Cap it at our maximum order (10)
    core::cmp::min(order, 10)
}

impl BuddyAllocator {
    pub fn new() -> Self {
        let free_list = core::array::from_fn(|_| None);
        BuddyAllocator { free_list }
    }

    pub fn add_free_region(&mut self, mut start_addr: usize, end_addr: usize) {
        while start_addr < end_addr {
            let remaining_pages = (end_addr - start_addr) >> 12;

            // Largest power of 2 that perfectly fits inside remaining_pages
            let max_fit_order = (63 - remaining_pages.leading_zeros()) as usize;

            // Largest order this physical address is aligned to support
            let max_align_order = if start_addr == 0 {
                10 // Max out if address is 0
            } else {
                (start_addr.trailing_zeros() as usize).saturating_sub(12)
            };

            // Take the strictest limit, capped at Order 10
            let current_order = core::cmp::min(core::cmp::min(max_fit_order, max_align_order), 10);

            let block = start_addr as *mut FreeBlock;

            unsafe {
                (*block).next = self.free_list[current_order].take();
                self.free_list[current_order] = Some(&mut *block);
            }

            // Advance the pointer by the exact byte size of the block we just carved
            start_addr += 1 << (12 + current_order);
        }
    }

    pub fn alloc(&mut self, order: usize) -> Option<&mut FreeBlock> {
        // Clamp the order to the maximum order
        let order = core::cmp::min(order, MAX_ORDER);
        let block = self.free_list[order].take();

        if let Some(block) = block {
            // Pop the block from the free list and return it
            self.free_list[order] = block.next.take();
            return Some(block);
        }
        // No free blocks of the requested order found, try higher orders
        for ord in order + 1..MAX_ORDER {
            let block = self.free_list[ord].take();

            if let Some(block) = block {
                // Pop the block from the free list
                self.free_list[ord] = block.next.take();
                // Split the block into lower order blocks
                let mut curr_order = ord;
                while curr_order > order {
                    // Decrement the order by 1 and split the block in half
                    curr_order -= 1;
                    let half_block_size = 1 << (12 + curr_order);
                    // Cast the block to a FreeBlock pointer
                    let buddy_addr = block as *mut FreeBlock as usize + half_block_size;
                    let buddy_block = buddy_addr as *mut FreeBlock;
                    // Push the block into the current order's free list
                    unsafe {
                        (*buddy_block).next = self.free_list[curr_order].take();
                        self.free_list[curr_order] = Some(&mut *buddy_block);
                    }
                }

                return Some(block);
            }
        }
        None
    }

    pub fn free(&mut self, mut addr: usize, mut order: usize) {
        // Clamp order just in case
        order = core::cmp::min(order, MAX_ORDER);

        while order < MAX_ORDER - 1 {
            let size = 1 << (12 + order);
            let buddy_addr = addr ^ size;
            let mut buddy_found = false;

            let mut curr_pointer =
                &mut self.free_list[order] as *mut Option<&'static mut FreeBlock>;

            while let Some(block) = unsafe { &mut *curr_pointer } {
                if *block as *mut FreeBlock as usize == buddy_addr {
                    // Found the buddy, remove it from the free list
                    unsafe {
                        *curr_pointer = block.next.take();
                    }
                    buddy_found = true;
                    break;
                }
                // Move the pointer to the next block
                curr_pointer = &mut block.next as *mut Option<&'static mut FreeBlock>;
            }

            if buddy_found {
                // Merge them! The merged address is ALWAYS the Left Buddy
                addr = buddy_addr & !size;
                order += 1;
            } else {
                // Buddy is not found, we can't merge anymore
                // Push current block into the free list
                unsafe {
                    let block_ptr = addr as *mut FreeBlock;
                    (*block_ptr).next = self.free_list[order].take();
                    self.free_list[order] = Some(&mut *block_ptr);
                }
                return;
            }
        }
        // If we reach MAX_ORDER, we just push it to the largest list
        unsafe {
            let block_ptr = addr as *mut FreeBlock;
            (*block_ptr).next = self.free_list[order].take();
            self.free_list[order] = Some(&mut *block_ptr);
        }
    }
}

#[repr(align(65536))]
struct AlignedMemory([u8; 65536]);

static mut FAKE_MEMORY: AlignedMemory = AlignedMemory([0; 65536]);

pub fn test_buddy_allocator() {
    crate::serial_println!("--- Starting Advanced Buddy Allocator Tests ---");

    let mut allocator = BuddyAllocator::new();
    let start_addr = unsafe { core::ptr::addr_of_mut!(FAKE_MEMORY) as usize };
    // 64 KB total = exactly sixteen 4KB blocks (Order 0)
    let end_addr = start_addr + 65536; 
    allocator.add_free_region(start_addr, end_addr);

    // ==========================================
    // TEST 1: Exhaustion & OOM
    // ==========================================
    crate::serial_println!("[Test 1] Memory Exhaustion");
    let mut blocks = [0usize; 16];
    for i in 0..16 {
        blocks[i] = allocator.alloc(0).expect("Failed to allocate valid memory") as *mut FreeBlock as usize;
    }
    crate::serial_println!("   Successfully allocated all 16 available 4KB pages.");

    let oom_block = allocator.alloc(0);
    if oom_block.is_none() {
        crate::serial_println!("   PASS: 17th allocation correctly returned None.");
    } else {
        panic!("   FAIL: Allocator invented memory that doesn't exist!");
    }

    // ==========================================
    // TEST 2: Out-of-Order Freeing
    // ==========================================
    crate::serial_println!("[Test 2] Out-of-Order Freeing & Deferred Merging");
    // We currently have 16 blocks allocated. Let's look at the first 4:
    // blocks[0] (A) and blocks[1] (B) are buddies.
    // blocks[2] (C) and blocks[3] (D) are buddies.

    allocator.free(blocks[0], 0); // Free A. Cannot merge, B is still taken.
    allocator.free(blocks[2], 0); // Free C. Cannot merge, D is still taken.
    allocator.free(blocks[3], 0); // Free D. Merges with C to form 8KB!
    
    // Now the ultimate test. Freeing B should merge with A to form 8KB. 
    // Then, the allocator should instantly realize the C+D 8KB buddy is ALSO free, 
    // and merge them all into a 16KB block!
    allocator.free(blocks[1], 0); 

    // If deferred merging worked, we should now be able to allocate an Order 2 (16KB) block.
    // We will do it in a match statement to prevent a hard panic if it fails.
    match allocator.alloc(2) {
        Some(ptr) => crate::serial_println!("   PASS: Deferred merge successful. Allocated 16KB block at {:#x}", ptr as *mut FreeBlock as usize),
        None => panic!("   FAIL: Allocator failed to merge out-of-order blocks!"),
    }

    // ==========================================
    // TEST 3: Impossible Demands
    // ==========================================
    crate::serial_println!("[Test 3] Impossible Allocation Requests");
    // Request Order 10 (4 MB). We only have a 64 KB pool.
    let massive_block = allocator.alloc(10);
    if massive_block.is_none() {
        crate::serial_println!("   PASS: Massive allocation correctly rejected.");
    } else {
        panic!("   FAIL: Allocator gave us memory it doesn't have.");
    }

    crate::serial_println!("--- All Advanced Edge Cases Passed! ---");
}
