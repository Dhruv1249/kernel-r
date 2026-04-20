// src/memory.rs

use crate::boot_info::MemoryMapEntry;

pub struct BumpAllocator {
    // Physical address of the next free frame
    next_free_frame: usize,
    // Static slice of memory map provided by GRUB
    memory_map: &'static [MemoryMapEntry],

    // Dynamic exclusion zones
    mbi_start: usize,
    mbi_end: usize,
    stack_start: usize,
    stack_end: usize,
}

impl BumpAllocator {
    pub fn init(
        kernel_end: usize,
        memory_map: &'static [MemoryMapEntry],
        mbi_start: usize,
        mbi_end: usize,
        stack_start: usize,
        stack_end: usize,
    ) -> Self {
        // Align kernel end to 4096 ie 4kb since a page frame must always be 4kb aligned
        // Its just ceil(kernel_end / 4096) * 4096
        let aligned_addr = (kernel_end + 4095) & !(4096 - 1);
        let next_free_frame = aligned_addr;
        BumpAllocator {
            next_free_frame,
            memory_map,
            mbi_start,
            mbi_end,
            stack_start,
            stack_end,
        }
    }

    pub fn allocate_frame(&mut self) -> Option<usize> {
        for mem in self.memory_map {
            if mem.typ == 1 {
                let region_base = mem.base_addr as usize;
                let region_end = region_base + mem.length as usize;

                // Pick the candidate address (whichever is higher)
                let mut candidate = region_base;
                if self.next_free_frame > candidate {
                    candidate = self.next_free_frame;
                }
                // --- COLLISION CHECKS ---
                // 1. IVT / BIOS area
                if candidate + 4096 > 0x0 && candidate < 0x1000 {
                    candidate = 0x1000;
                }
                // 2. VGA / BIOS reserved area
                if candidate + 4096 > 0xA0000 && candidate < 0x100000 {
                    candidate = 0x100000;
                }
                // 3. Multiboot Info
                if candidate + 4096 > self.mbi_start && candidate < self.mbi_end {
                    candidate = self.mbi_end;
                }
                // 4. Boot Stack
                if candidate + 4096 > self.stack_start && candidate < self.stack_end {
                    candidate = self.stack_end;
                }
                //Align the candidate to a 4KB boundary
                candidate = (candidate + 4095) & !(4096 - 1);

                // Check if a full 4096-byte frame fits in the remaining space
                if candidate + 4096 <= region_end {
                    self.next_free_frame = candidate + 4096;
                    return Some(candidate);
                }
            }
        }
        None
    }

    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<usize> {
        for mem in self.memory_map {
            if mem.typ == 1 {
                let region_base = mem.base_addr as usize;
                let region_end = region_base + mem.length as usize;

                // Pick the candidate address (whichever is higher)
                let mut candidate = region_base;
                if self.next_free_frame > candidate {
                    candidate = self.next_free_frame;
                }
                // --- COLLISION CHECKS ---
                // 1. IVT / BIOS area
                if candidate + 4096 * count > 0x0 && candidate < 0x1000 {
                    candidate = 0x1000;
                }
                // 2. VGA / BIOS reserved area
                if candidate + 4096 * count > 0xA0000 && candidate < 0x100000 {
                    candidate = 0x100000;
                }
                // 3. Multiboot Info
                if candidate + 4096 * count > self.mbi_start && candidate < self.mbi_end {
                    candidate = self.mbi_end;
                }
                // 4. Boot Stack
                if candidate + 4096 * count > self.stack_start && candidate < self.stack_end {
                    candidate = self.stack_end;
                }
                //Align the candidate to a 4KB boundary
                candidate = (candidate + 4095) & !(4096 - 1);

                if candidate + 4096 * count <= region_end {
                    self.next_free_frame = candidate + 4096 * count;
                    return Some(candidate);
                }
            }
        }
        None
    }
}

use spin::Mutex;

// Start as None we will fill it at runtime
pub static ALLOCATOR: Mutex<Option<BumpAllocator>> = Mutex::new(None);

pub fn allocate_frame() -> Option<usize> {
    let mut lock = ALLOCATOR.lock();

    if let Some(allocator) = lock.as_mut() {
        allocator.allocate_frame()
    } else {
        None
    }
}

// Allocates a 4KB physical frame and completely zeroes it out.
pub fn allocate_zeroed_frame() -> Option<usize> {
    // Get a raw frame from our standard allocator
    let frame_addr = allocate_frame()?; // The '?' returns None if out of memory

    // Cast the usize address into a raw mutable byte pointer
    let ptr = frame_addr as *mut u8;

    // Zero out exactly 4096 bytes
    unsafe {
        core::ptr::write_bytes(ptr, 0, 4096);
    }

    Some(frame_addr)
}

pub struct BitmapAllocator {
    bitmap_ptr: *mut u8,
    total_frames: usize,
}

impl BitmapAllocator {
    pub fn init(memory_map: &'static [MemoryMapEntry], bump_alloc: &mut BumpAllocator) -> Self {
        let mut highest_addr = 0;
        // We need to find the highest address in the memory map
        // Since bitmap will cover the entire memory map even including our kernel and reserved areas
        for mem in memory_map {
            let region_end = (mem.base_addr + mem.length) as usize;
            if region_end > highest_addr {
                highest_addr = region_end;
            }
        }
        // Convert the highest address to the no of 4kb frames
        let total_frames = highest_addr / 4096;

        // In bitmap 1 bit = 1 frame so convert bytes to bits
        let bitmap_size_in_bytes = total_frames / 8;

        // Round up to the next 4kb frame
        let frames_for_bitmap = (bitmap_size_in_bytes + 4095) / 4096;

        // Allocate frames for the bitmap using our bump allocator
        let frames = bump_alloc.allocate_contiguous_frames(frames_for_bitmap);

        // If we can't allocate frames for the bitmap then panic
        if frames.is_none() {
            panic!("Not enough contiguous frames to allocate bitmap");
        }
        let bitmap_ptr = frames.unwrap() as *mut u8;

        // Setting all frames to used
        use core::ptr::write_bytes;
        unsafe {
            write_bytes(bitmap_ptr, 0xFF, bitmap_size_in_bytes);
        }

        let mut allocator = BitmapAllocator {
            bitmap_ptr,
            total_frames,
        };

        for mem in memory_map {
            if mem.typ == 1 {
                let region_base = mem.base_addr as usize;
                let region_end = region_base + mem.length as usize;
                
                // Convert address to frame indices
                let region_base = region_base / 4096;
                let region_end = region_end / 4096;

                for i in region_base..region_end {
                    allocator.clear_bit(i);
                }
            }
        }

        allocator
    }

    pub fn set_bit(&mut self, index: usize) {
        let byte_index = index / 8;
        let bit_offset = index % 8;

        // Create a mask with a 1 in the exact position we want to change
        let mask = 1 << bit_offset;

        unsafe {
            // Get the pointer to the byte
            let byte_ptr = self.bitmap_ptr.add(byte_index);

            // Read the byte, OR it with the mask, and write it back
            // Example: 01000000 | 00000100 = 01000100
            *byte_ptr = *byte_ptr | mask;
        }
    }

    pub fn clear_bit(&mut self, index: usize) {
        let byte_index = index / 8;
        let bit_offset = index % 8;

        // Create a mask with a 0 in the exact position, and 1s everywhere else
        let mask = !(1 << bit_offset);

        unsafe {
            // Get the pointer to the byte
            let byte_ptr = self.bitmap_ptr.add(byte_index);

            // Read the byte, AND it with the inverted mask, and write it back
            // Example: 01000100 & 11111011 = 01000000
            *byte_ptr = *byte_ptr & mask;
        }
    }
}
