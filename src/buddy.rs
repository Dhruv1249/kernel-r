// src/buddy.rs

pub struct FreeBlock {
    next: *mut FreeBlock,
    prev: *mut FreeBlock,
}

impl FreeBlock {
    // A clean way to initialize an empty block
    pub const fn empty() -> Self {
        FreeBlock {
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        }
    }
}

const MAX_ORDER: usize = 11; // Order 0 to 10

pub struct BuddyAllocator {
    // Array of 11 linked lists. Index 0 is 4KB, Index 1 is 8KB, etc. upto 4MB
    free_list: [*mut FreeBlock; MAX_ORDER],
    bitmap: *mut u8,
    bitmap_size: usize,
    base_addr: usize,
}

pub fn size_to_order(size_in_bytes: usize) -> usize {
    // Add 4095 to ceiling the value, then bit-shift right by 12 to divide by 4096
    let pages = (size_in_bytes + 4095) >> 12;

    // The CPU natively calculates the next power of 2 and counts the zeros
    let order = pages.next_power_of_two().trailing_zeros() as usize;

    // Cap it at our maximum order (10)
    core::cmp::min(order, 10)
}

pub fn calculate_bitmap_size(highest_physical_address: usize) -> usize {
    // In this bitmap 1 bit = 1 buddy (2 frames)
    // Divide by 65,536 (2^16) to find how many bytes the bitmap needs
    let raw_bytes = highest_physical_address >> 16;
    // Round up to the next 4kb frame
    (raw_bytes + 4095) & !(4096 - 1)
}

impl BuddyAllocator {
    pub fn new(bitmap_ptr: *mut u8, bitmap_size: usize, base_addr: usize) -> Self {
        // let mut free_list = [FreeBlock::empty(),MAX_ORDER];
        BuddyAllocator {
            free_list: [core::ptr::null_mut(); MAX_ORDER],
            bitmap: bitmap_ptr,
            bitmap_size,
            base_addr
        }
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

            self.push_block(current_order, block);
            self.toggle_bit(current_order, start_addr);

            // Advance the pointer by the exact byte size of the block we just carved
            start_addr += 1 << (12 + current_order);
        }
    }

    pub fn alloc(&mut self, order: usize) -> Option<*mut FreeBlock> {
        // Clamp the order to the maximum order
        let order = core::cmp::min(order, MAX_ORDER);
        let block = self.free_list[order];

        if block != core::ptr::null_mut() {
            // Pop the block from the free list and return it
            self.remove_block(order, block);
            self.toggle_bit(order, block as usize);
            return Some(block);
        }
        // No free blocks of the requested order found, try higher orders
        for ord in order + 1..MAX_ORDER {
            let block = self.free_list[ord];

            if block != core::ptr::null_mut() {
                // Pop the block from the free list
                self.remove_block(ord, block);
                self.toggle_bit(ord, block as usize);
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
                    self.push_block(curr_order, buddy_block);
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
            // Toggle the bit. If it returns true, our buddy is also free!
            let buddy_is_free = self.toggle_bit(order, addr);

            if buddy_is_free {
                //  Calculate the buddy's address
                let size = 1 << (12 + order);
                let buddy_addr = addr ^ size;

                //  Rip the buddy out of the free list
                self.remove_block(order, buddy_addr as *mut FreeBlock);

                //  Merge them (address becomes the Left Buddy)
                addr = addr & !size;
                order += 1;
            } else {
                // Buddy is allocated. We are done merging.
                // Push this block to the list and stop.
                self.push_block(order, addr as *mut FreeBlock);
                self.toggle_bit(order, addr);
                return;
            }
        }
        // At MAX_ORDER, we just push it. No buddy exists at Order 11.
        self.push_block(order, addr as *mut FreeBlock);
    }

    pub fn push_block(&mut self, order: usize, block: *mut FreeBlock) {
        let head = self.free_list[order];
        unsafe {
            (*block).prev = core::ptr::null_mut();
            (*block).next = head; // Point next to the current head (even if it's null!)

            if !head.is_null() {
                (*head).prev = block; // Wire the old head backward to our new block
            }
        }
        // Update the head of the list
        self.free_list[order] = block;
    }

    pub fn remove_block(&mut self, order: usize, block: *mut FreeBlock) {
        unsafe {
            if (*block).prev != core::ptr::null_mut() {
                (*(*block).prev).next = (*block).next;
            } else {
                self.free_list[order] = (*block).next;
            }
            if (*block).next != core::ptr::null_mut() {
                (*(*block).next).prev = (*block).prev;
            }
        }
    }

   pub fn toggle_bit(&mut self, order: usize, addr: usize) -> bool {
        // Convert the virtual memory pointer back into a 0-based physical offset
        let phys_addr = addr.saturating_sub(self.base_addr); 
        
        let mut bit_index = phys_addr >> (12 + order + 1);
        
        // Add offset so they won't overlap
        bit_index += order * ((self.bitmap_size / 11) * 8);
        
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        let mask = 1 << bit_offset;
        
        unsafe {
            let byte_ptr = self.bitmap.add(byte_index);
            *byte_ptr ^= mask;
            (*byte_ptr & mask) == 0
        }
    }
}

#[repr(align(65536))]
struct AlignedMemory([u8; 65536]);

static mut FAKE_MEMORY: AlignedMemory = AlignedMemory([0; 65536]);
// Add a fake bitmap array for testing
static mut FAKE_BITMAP: AlignedMemory = AlignedMemory([0; 65536]);

pub fn test_buddy_allocator() {
    crate::serial_println!("--- Starting O(1) Buddy Allocator Tests ---");

    let start_addr = unsafe { core::ptr::addr_of_mut!(FAKE_MEMORY.0) as usize };
    let end_addr = start_addr + 65536; 

    let bitmap_ptr = unsafe { core::ptr::addr_of_mut!(FAKE_BITMAP.0) as *mut u8 };
    
    // Pass start_addr as our physical base!
    let mut allocator = BuddyAllocator::new(bitmap_ptr, 65536, start_addr);

    allocator.add_free_region(start_addr, end_addr);

    // ==========================================
    // TEST 1: Exhaustion & OOM
    // ==========================================
    crate::serial_println!("[Test 1] Memory Exhaustion");
    let mut blocks = [0usize; 16];
    for i in 0..16 {
        blocks[i] = allocator.alloc(0).expect("Failed to allocate") as *mut FreeBlock as usize;
    }
    crate::serial_println!("   PASS: Allocated all 16 pages.");

    if allocator.alloc(0).is_none() {
        crate::serial_println!("   PASS: 17th allocation correctly returned None.");
    } else {
        panic!("   FAIL: Allocator invented memory!");
    }

    // ==========================================
    // TEST 2: Out-of-Order Freeing
    // ==========================================
    crate::serial_println!("[Test 2] Out-of-Order Freeing & Deferred Merging");
    allocator.free(blocks[0], 0); 
    allocator.free(blocks[2], 0); 
    allocator.free(blocks[3], 0); 
    allocator.free(blocks[1], 0); 

    match allocator.alloc(2) {
        Some(ptr) => crate::serial_println!("   PASS: Deferred merge successful. Allocated 16KB block at {:#x}", ptr as *mut FreeBlock as usize),
        None => panic!("   FAIL: Allocator failed to merge out-of-order blocks!"),
    }

    // ==========================================
    // TEST 3: Impossible Demands
    // ==========================================
    crate::serial_println!("[Test 3] Impossible Allocation Requests");
    if allocator.alloc(10).is_none() {
        crate::serial_println!("   PASS: Massive allocation correctly rejected.");
    } else {
        panic!("   FAIL: Allocator gave us memory it doesn't have.");
    }

    crate::serial_println!("--- All O(1) Edge Cases Passed! ---");
}
