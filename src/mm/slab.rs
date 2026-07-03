// src/mm/slab.rs

/// The fixed block sizes (in bytes) managed by the slab allocator.
///
/// Each entry corresponds to one *slab class*.  When an allocation request
/// arrives, the allocator picks the smallest class that fits the request.
/// Sizes are powers of two starting at 8 bytes, topping out at 2048 bytes;
/// anything larger falls back to the buddy allocator.
const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 256, 512, 1024, 2048];

/// An intrusive linked-list node stored in-place inside a free slab block.
///
/// When a block is free, its first bytes are reinterpreted as a `ListNode`
/// pointing to the next free block in the same slab class.  No separate
/// metadata allocation is needed because the free block's own memory is used
/// to hold the pointer.
struct ListNode {
    next: *mut ListNode,
}

/// A multi-class slab allocator for small kernel heap objects.
///
/// # Design
///
/// The slab allocator maintains one free list per size class in `BLOCK_SIZES`.
/// Each free list is a singly-linked list threaded through the free blocks
/// themselves (via `ListNode`).
///
/// On the **fast path** (cache hit) allocation is O(1): pop the head of the
/// matching free list and return it.
///
/// On a **cache miss** (free list empty) the caller is expected to obtain a
/// fresh 4 KiB page from the virtual bump allocator, pass it to
/// [`SlabAllocator::populate_cache`], and retry.  `populate_cache` splits the
/// page into `4096 / block_size` blocks, links them into a chain, and returns
/// the first block directly to the caller.
///
/// **Deallocation** is always O(1): push the block back onto the head of its
/// class's free list.
pub struct SlabAllocator {
    // Array of list heads
    list_heads: [*mut ListNode; BLOCK_SIZES.len()],
}

impl SlabAllocator {
    /// Creates a new `SlabAllocator` with all free lists empty.
    ///
    /// This is a `const` function so it can be used to initialise a static
    /// `ALLOCATOR` before the heap is available.
    pub const fn new() -> Self {
        SlabAllocator {
            list_heads: [core::ptr::null_mut(); BLOCK_SIZES.len()],
        }
    }

    /// Returns the index into `BLOCK_SIZES` that can satisfy an allocation of `size` bytes.
    ///
    /// Iterates through `BLOCK_SIZES` and returns the index of the first entry
    /// that is `>= size`.  Returns `None` if `size` exceeds the largest slab
    /// class (2048 bytes), signalling that the request must go to the buddy
    /// allocator instead.
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

    /// Attempts to allocate a block of at least `size` bytes from the slab cache.
    ///
    /// Returns `Some(ptr)` if a free block is available in the matching slab
    /// class (fast path), or `None` if the cache is empty (cache miss — the
    /// caller must call [`SlabAllocator::populate_cache`] first).
    ///
    /// Returns `None` also if `size` exceeds 2048 bytes, as there is no
    /// matching slab class for it.
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

    /// Splits a fresh 4 KiB page into slab blocks and links them into the free list.
    ///
    /// # How it works
    ///
    /// Given a newly-obtained page at `page_ptr` and the slab class `index`,
    /// the function divides the 4096-byte page into `4096 / BLOCK_SIZES[index]`
    /// equal blocks.  It writes a `ListNode` pointer at the start of each block
    /// pointing to the next block, forming a singly-linked chain.  The last
    /// block points to `null`.
    ///
    /// The **first block** is returned to the caller directly (it is already
    /// allocated).  The remaining blocks (starting from block 1) become the
    /// new head of `list_heads[index]`, immediately satisfying future
    /// allocations without another page request.
    ///
    /// # Safety
    /// - `page_ptr` must point to a valid, exclusively-owned, writable 4 KiB region.
    /// - `index` must be a valid index into `BLOCK_SIZES`.
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

    /// Returns a previously-allocated slab block back to the free list.
    ///
    /// Casts `ptr` to a `ListNode` and prepends it to the head of the
    /// `list_heads[index]` list corresponding to `size`.  This is O(1).
    ///
    /// # Panics
    /// Panics if `size` exceeds 2048 bytes, which indicates a programming error —
    /// blocks that large should never have been allocated by the slab and should
    /// be freed through the buddy allocator.
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
