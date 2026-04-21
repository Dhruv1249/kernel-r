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
    kernel_end: usize,
}

pub struct exclusion_zones {
    mbi_start: usize,
    mbi_end: usize,
    stack_start: usize,
    stack_end: usize,
    kernel_end: usize,
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
            kernel_end: next_free_frame,
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

    pub fn get_exclusion_zones(&self) -> exclusion_zones {
        exclusion_zones {
            mbi_start: self.mbi_start,
            mbi_end: self.mbi_end,
            stack_start: self.stack_start,
            stack_end: self.stack_end,
            kernel_end: self.kernel_end,
        }
    }
}

use spin::Mutex;

// Start as None we will fill it at runtime
pub static ALLOCATOR: Mutex<Option<BitmapAllocator>> = Mutex::new(None);

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

// Letting compiler know that we will be using this struct in a multithreaded environment
unsafe impl Send for BitmapAllocator {}

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

        // Mark the usable memory as free
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

        // Rereserving exclusion zones

        let exclusion_zones = bump_alloc.get_exclusion_zones();

        let stack_start_frame = exclusion_zones.stack_start / 4096;
        let stack_end_frame = (exclusion_zones.stack_end + 4095) / 4096;
        let mbi_start_frame = exclusion_zones.mbi_start / 4096;
        let mbi_end_frame = (exclusion_zones.mbi_end + 4095) / 4096;

        // Reserve frames for multiboot info
        for i in mbi_start_frame..mbi_end_frame {
            allocator.set_bit(i);
        }

        // Reserve frames for boot stack
        for i in stack_start_frame..stack_end_frame {
            allocator.set_bit(i);
        }

        // Reserve frames for kernel
        let kernel_start_frame = 0;
        let kernel_end_frame = (exclusion_zones.kernel_end + 4095) / 4096;

        for i in kernel_start_frame..kernel_end_frame {
            allocator.set_bit(i);
        }

        // Set bits for bitmap itself
        let bitmap_start_frame = frames.unwrap() / 4096;
        let bitmap_end_frame = (frames.unwrap() + bitmap_size_in_bytes + 4095) / 4096;

        for i in bitmap_start_frame..bitmap_end_frame {
            allocator.set_bit(i);
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

    pub fn is_bit_free(&self, index: usize) -> bool {
        let byte_index = index / 8;
        let bit_offset = index % 8;

        // Create a mask with a 1 in the exact position, and 0s everywhere else
        let mask = 1 << bit_offset;

        unsafe {
            // Get the pointer to the byte
            let byte_ptr = self.bitmap_ptr.add(byte_index);

            // Read the byte, AND it with the mask
            // Example: 01000100 & 00000100 = 00000100
            // If the result is 0, then the bit is free
            *byte_ptr & mask == 0
        }
    }

    pub fn allocate_frame(&mut self) -> Option<usize> {
        for i in 0..self.total_frames {
            if self.is_bit_free(i) {
                // Free frame
                self.set_bit(i); // Set the bit to 1 to mark it as used
                return Some(i * 4096); // Return the frame address
            }
        }
        None // Out of memory
    }
}

// Comically large number easy to debug
pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 1024 * 100; // 100 KB

pub fn init_heap(p4_table: &mut x86_64::structures::paging::PageTable) {
    let heap_start_page = x86_64::structures::paging::page::Page::containing_address(
        x86_64::VirtAddr::new(HEAP_START as u64),
    );
    let heap_end_page = x86_64::structures::paging::page::Page::containing_address(
        x86_64::VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64),
    );
    let heap_range =
        x86_64::structures::paging::Page::range_inclusive(heap_start_page, heap_end_page);

    for page in heap_range {
        let frame = allocate_frame();
        if frame.is_none() {
            panic!("Out of memory in heap");
        }
        let frame_addr = frame.unwrap();

        let phys_address = x86_64::PhysAddr::new(frame_addr as u64);
        let flags = x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE;
        let physical_frame = x86_64::structures::paging::PhysFrame::containing_address(phys_address);
        

        crate::paging::map_to(page, physical_frame, flags, p4_table);
    }
}
