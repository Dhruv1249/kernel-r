// src/mm/buddy.rs

/// A node in a buddy-allocator free list, stored in-place inside free memory.
///
/// When a physical memory block is free, its first bytes are reinterpreted as a
/// `FreeBlock`.  The block forms part of a doubly-linked list so that
/// removal (during allocation or merging) is O(1) regardless of list length.
///
/// # Invariants
/// - `next` and `prev` are either null or aligned pointers to other `FreeBlock`s.
/// - The memory backing the node must not be used for any other purpose while
///   it is in a free list.
pub struct FreeBlock {
    next: *mut FreeBlock,
    prev: *mut FreeBlock,
}

impl FreeBlock {
    /// Returns a sentinel `FreeBlock` with both pointers set to null.
    ///
    /// Used to initialise the placeholder elements at the head of each free
    /// list before any memory has been added.
    // A clean way to initialize an empty block
    pub const fn empty() -> Self {
        FreeBlock {
            next: core::ptr::null_mut(),
            prev: core::ptr::null_mut(),
        }
    }
}

/// The number of distinct block-size classes tracked by the buddy allocator.
///
/// Order 0 = 4 KiB (1 page), Order 1 = 8 KiB, …, Order 10 = 4 MiB.
/// Eleven orders covers the entire practical range for kernel heap objects.
const MAX_ORDER: usize = 11; // Order 0 to 10

/// A binary-buddy physical frame allocator.
///
/// # Design
///
/// The buddy system partitions physical memory into power-of-two-sized blocks
/// called *buddies*.  Each order `n` manages blocks of `4096 × 2ⁿ` bytes.
/// When a block of order `n` is freed, the allocator checks whether its
/// *buddy* (the adjacent, same-size block that together would form an
/// order-`n+1` block) is also free.  If so, the two buddies are merged into
/// one larger block and the process repeats recursively up to `MAX_ORDER - 1`.
///
/// # Bitmap
/// A compact bitmap tracks which buddy pairs are split.  Each bit represents
/// one pair of buddies at a given order: bit = 0 means both buddies have the
/// same allocation state (both free or both allocated); bit = 1 means they
/// differ.  Toggling the bit on alloc and free gives O(1) buddy-state lookup
/// without any additional metadata per block.
///
/// # Free lists
/// An array of `MAX_ORDER` singly-headed doubly-linked lists stores the free
/// blocks at each order.  Blocks are stored in-place — the `FreeBlock` header
/// is written into the first bytes of the free physical frame.
///
/// # Safety
/// All pointer arithmetic is inherently unsafe.  The allocator is wrapped in
/// `crate::mm::allocator::Locked<T>` to serialise access.
pub struct BuddyAllocator {
    // Array of 11 linked lists. Index 0 is 4KB, Index 1 is 8KB, etc. upto 4MB
    free_list: [*mut FreeBlock; MAX_ORDER],
    bitmap: *mut u8,
    bitmap_size: usize,
    base_addr: usize,
}

/// Converts a byte size to the minimum buddy order that can satisfy it.
///
/// The order is computed by ceiling-dividing `size_in_bytes` by 4096 (page
/// size) to get a page count, then taking the next power-of-two's trailing
/// zeros as the order.  The result is capped at order 10 (4 MiB) to stay
/// within the allocator's `MAX_ORDER`.
///
/// # Examples
/// - 4096 bytes → order 0 (1 page)
/// - 8192 bytes → order 1 (2 pages)
/// - 5000 bytes → order 1 (rounded up to 2 pages)
pub fn size_to_order(size_in_bytes: usize) -> usize {
    // Add 4095 to ceiling the value, then bit-shift right by 12 to divide by 4096
    let pages = (size_in_bytes + 4095) >> 12;

    // The CPU natively calculates the next power of 2 and counts the zeros
    let order = pages.next_power_of_two().trailing_zeros() as usize;

    // Cap it at our maximum order (10)
    core::cmp::min(order, 10)
}

/// Computes the number of bytes required for the buddy bitmap.
///
/// The bitmap needs one bit per buddy pair at order 0 (the finest granularity).
/// Each buddy pair covers `2 × 4096 = 65536` bytes (2^16), so the number of
/// bits equals `highest_physical_address >> 16`.  The result is rounded up to
/// the nearest 4 KiB page so the bitmap itself can be allocated in whole pages.
pub fn calculate_bitmap_size(highest_physical_address: usize) -> usize {
    // In this bitmap 1 bit = 1 buddy (2 frames)
    // Divide by 65,536 (2^16) to find how many bytes the bitmap needs
    let raw_bytes = highest_physical_address >> 16;
    // Round up to the next 4kb frame
    (raw_bytes + 4095) & !(4096 - 1)
}

impl BuddyAllocator {

    /// Reinitialises an already-constructed allocator with a new bitmap and base address.
    ///
    /// This is the companion to [`BuddyAllocator::empty`]: after calling `empty`
    /// to create a placeholder and storing it in a static, call `init` once the
    /// bitmap memory is available to make the allocator operational.
    ///
    /// # Safety
    /// - `bitmap_ptr` must point to `bitmap_size` bytes of zeroed, exclusively-owned memory.
    /// - `base_addr` must equal the virtual offset added to all physical addresses
    ///   before they are handed to this allocator (i.e. `PHYS_OFFSET`).
    pub unsafe fn init(&mut self, bitmap_ptr: *mut u8, bitmap_size: usize, base_addr: usize) {
        self.bitmap = bitmap_ptr;
        self.bitmap_size = bitmap_size;
        self.base_addr = base_addr;
        self.free_list = [core::ptr::null_mut(); MAX_ORDER];
    }

    /// Creates a fully-initialised `BuddyAllocator` with the given bitmap.
    ///
    /// Prefer [`BuddyAllocator::empty`] + [`BuddyAllocator::init`] when the
    /// allocator must live in a static (which requires a `const` constructor).
    pub fn new(bitmap_ptr: *mut u8, bitmap_size: usize, base_addr: usize) -> Self {
        BuddyAllocator {
            free_list: [core::ptr::null_mut(); MAX_ORDER],
            bitmap: bitmap_ptr,
            bitmap_size,
            base_addr
        }
    }

    /// Creates a placeholder `BuddyAllocator` that is safe to store in a static.
    ///
    /// All pointers are null and sizes are zero.  The allocator must be
    /// initialised with [`BuddyAllocator::init`] before any allocation is
    /// attempted, otherwise the first `alloc` call will return `None`.
    pub const fn empty() -> Self {
        BuddyAllocator {
            free_list: [core::ptr::null_mut(); MAX_ORDER],
            bitmap: core::ptr::null_mut(),
            bitmap_size: 0,
            base_addr: 0,
        }
    }

    /// Feeds a contiguous range of free physical addresses into the allocator.
    ///
    /// The range `[start_addr, end_addr)` is carved into the largest possible
    /// naturally-aligned buddy blocks.  For each iteration the algorithm picks
    /// the minimum of:
    /// - the largest power-of-two order that fits in the remaining range, and
    /// - the largest order that `start_addr`'s alignment can support.
    ///
    /// The chosen block is pushed onto the appropriate free list and its bitmap
    /// bit is toggled to mark it as free, then `start_addr` is advanced past it.
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

    /// Allocates a block of the given `order` (i.e. `4096 × 2^order` bytes).
    ///
    /// # Strategy
    /// 1. If the exact-order free list is non-empty, pop and return the head block.
    /// 2. Otherwise, search upward through higher orders until a block is found.
    /// 3. Split that block down to the requested order by repeatedly halving it,
    ///    pushing the upper halves (buddies) onto their respective free lists.
    ///
    /// Returns `None` if no block large enough is available.
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

    /// Toggles the bitmap bit for the buddy pair that contains `addr` at `order`.
    ///
    /// # Bitmap layout
    /// The bitmap uses a geometric-series partition:
    /// - Order 0 gets the first half of the bitmap bits.
    /// - Order 1 gets the next quarter, etc.
    ///
    /// This is computed via a cumulative `offset` before adding the per-order
    /// bit index.  After toggling, the function returns `true` if the bit is
    /// now 0 (meaning both buddies are free — a merge is possible).
    ///
    /// # Panics
    /// Panics if the computed byte index falls outside `bitmap_size`, which
    /// indicates a serious memory-layout bug.
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

    /// Returns a previously-allocated block at `addr` of the given `order` to the free pool.
    ///
    /// # Merging (coalescing)
    /// After marking the block as free, the allocator tries to merge it with its
    /// buddy.  It does this by toggling the shared bitmap bit: if the result is 0
    /// (both buddies now free) the buddy is removed from its free list and the
    /// two are coalesced into a block of `order + 1`, then the process repeats.
    /// If the bit is 1 (buddy still allocated) the loop stops and the block is
    /// pushed onto the current-order free list.
    ///
    /// The XOR trick `addr ^ size` computes the buddy address because buddies
    /// are always aligned to `2 × size` and differ only in bit `log2(size)`.
    pub fn free(&mut self, mut addr: usize, mut order: usize) {
        // Unconditionally strip the lower 12 bits (hardware flags) to ensure perfect 4KB alignment.
        addr &= !0xFFF;
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

    /// Inserts `block` at the head of the order-`order` free list in O(1).
    ///
    /// Updates the old head's `prev` pointer to maintain the doubly-linked
    /// list invariant.  The new block becomes the new head.
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

    /// Removes `block` from the middle (or head) of the order-`order` free list in O(1).
    ///
    /// Stitches `block->prev->next` and `block->next->prev` together, bypassing
    /// the removed node.  If `block` is the list head, updates `free_list[order]`
    /// directly.
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


    /// Walks the internal free lists and aggregates total available bytes.
    pub fn count_free_memory(&self) -> usize {
        let mut total_free = 0;

        for order in 0..MAX_ORDER {
            let mut curr = self.free_list[order];
            let mut block_count = 0;

            while !curr.is_null() {
                block_count += 1;
                unsafe {
                    curr = (*curr).next;
                }
            }
            
            // Order 0 = 4096 << 0 (4KB), Order 1 = 4096 << 1 (8KB), etc.
            total_free += block_count * (4096 << order);
        }

        total_free
    }
}

unsafe impl Send for BuddyAllocator {}
unsafe impl Sync for BuddyAllocator {}
