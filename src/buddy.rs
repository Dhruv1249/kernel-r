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

    pub unsafe fn init(&mut self, bitmap_ptr: *mut u8, bitmap_size: usize, base_addr: usize) {
        self.bitmap = bitmap_ptr;
        self.bitmap_size = bitmap_size;
        self.base_addr = base_addr;
        self.free_list = [core::ptr::null_mut(); MAX_ORDER];
    }

    pub fn new(bitmap_ptr: *mut u8, bitmap_size: usize, base_addr: usize) -> Self {
        BuddyAllocator {
            free_list: [core::ptr::null_mut(); MAX_ORDER],
            bitmap: bitmap_ptr,
            bitmap_size,
            base_addr
        }
    }

    pub const fn empty() -> Self {
        BuddyAllocator {
            free_list: [core::ptr::null_mut(); MAX_ORDER],
            bitmap: core::ptr::null_mut(),
            bitmap_size: 0,
            base_addr: 0,
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
        let order = core::cmp::min(order, MAX_ORDER - 1);
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
                    
                    self.toggle_bit(curr_order, buddy_addr);
                }

                return Some(block);
            }
        }
        None
    }

    pub fn toggle_bit(&mut self, order: usize, addr: usize) -> bool {
        let phys_addr = addr.saturating_sub(self.base_addr); 
        let mut bit_index = phys_addr >> (12 + order + 1);
        
        // Geometric series offset. 
        // Order 0 gets half the bitmap, Order 1 gets a quarter, etc.
        let mut offset = 0;
        for i in 0..order {
            offset += (self.bitmap_size * 8) >> (i + 1);
        }
        bit_index += offset;
        
        let byte_index = bit_index / 8;
        let bit_offset = bit_index % 8;
        let mask = 1 << bit_offset;

        if byte_index >= self.bitmap_size {
            panic!("FATAL: Buddy Allocator out of bounds write! Addr: {:#x}, Order: {}", addr, order);
        }
        
        unsafe {
            let byte_ptr = self.bitmap.add(byte_index);
            *byte_ptr ^= mask;
            (*byte_ptr & mask) == 0
        }
    }

    pub fn free(&mut self, mut addr: usize, mut order: usize) {
        order = core::cmp::min(order, MAX_ORDER - 1);

        while order < MAX_ORDER - 1 {
            let buddy_is_free = self.toggle_bit(order, addr);

            if buddy_is_free {
                // Buddy is free! Merge them.
                let size = 1 << (12 + order);
                let buddy_addr = addr ^ size;
                self.remove_block(order, buddy_addr as *mut FreeBlock);
                addr = addr & !size;
                order += 1;
            } else {
                // Buddy is allocated. We are done merging.
                self.push_block(order, addr as *mut FreeBlock);
                return;
            }
        }
        // At MAX_ORDER, we just push it.
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
}

unsafe impl Send for BuddyAllocator {}
unsafe impl Sync for BuddyAllocator {}
