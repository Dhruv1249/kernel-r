// src/memory.rs

// Start as an empty allocator, initialized fully at boot
pub static FRAME_ALLOCATOR: crate::allocator::Locked<crate::buddy::BuddyAllocator> = crate::allocator::Locked::new(crate::buddy::BuddyAllocator::empty());

pub const HEAP_START: usize = 0x_4444_4444_0000;
pub const HEAP_SIZE: usize = 0xA00000; // 10 MB

#[derive(Copy, Clone)]
pub struct ReservedRegion {
    pub start: usize,
    pub end: usize,
    pub name: &'static str,
}

const MAX_RESERVED: usize = 32;
pub static mut RESERVED_REGIONS: [ReservedRegion; MAX_RESERVED] = [ReservedRegion {
    start: 0,
    end: 0,
    name: "",
}; MAX_RESERVED];
pub static mut RESERVED_COUNT: usize = 0;

pub unsafe fn reserve_region(start: usize, end: usize, name: &'static str) {
    unsafe {
        if RESERVED_COUNT < MAX_RESERVED {
            RESERVED_REGIONS[RESERVED_COUNT] = ReservedRegion { start, end, name };
            RESERVED_COUNT += 1;
            crate::serial_println!("Reserved: {} [{:#x} - {:#x}]", name, start, end);
        } else {
            panic!("Out of reserved region slots!");
        }
    }
}

pub fn is_reserved(phys_addr: usize, size: usize) -> bool {
    let end_addr = phys_addr + size;
    unsafe {
        for i in 0..RESERVED_COUNT {
            let region = &RESERVED_REGIONS[i];
            // Check for overlap
            if phys_addr < region.end && end_addr > region.start {
                return true;
            }
        }
    }
    false
}

// BUMP ALLOCATOR (For bootstrapping only)
pub struct BumpAllocator {
    next_free_frame: usize,
    memory_map: &'static [crate::boot_info::MemoryMapEntry],
}

impl BumpAllocator {
    pub fn init(kernel_end: usize, memory_map: &'static [crate::boot_info::MemoryMapEntry]) -> Self {
        let aligned_addr = (kernel_end + 4095) & !(4096 - 1);
        BumpAllocator {
            next_free_frame: aligned_addr,
            memory_map,
        }
    }

    pub fn allocate_contiguous_frames(&mut self, count: usize) -> Option<usize> {
        let size = count * 4096;
        for mem in self.memory_map {
            if mem.typ == 1 {
                let region_base = mem.base_addr as usize;
                let region_end = region_base + mem.length as usize;

                let mut candidate = region_base;
                if self.next_free_frame > candidate {
                    candidate = self.next_free_frame;
                }

                candidate = (candidate + 4095) & !(4096 - 1);

                // Scan forward to find a contiguous block that isn't reserved
                'search: while candidate + size <= region_end {
                    // Check if any frame in this requested block overlaps a reserved region
                    for i in 0..count {
                        let check_addr = candidate + (i * 4096);
                        if is_reserved(check_addr, 4096) {
                            // Hit a reserved block, jump past it and try again
                            candidate = (check_addr + 4096) & !(4096 - 1);
                            continue 'search;
                        }
                    }

                    // We found a completely free, unreserved contiguous block!
                    self.next_free_frame = candidate + size;
                    return Some(candidate);
                }
            }
        }
        None
    }
}

// SYSTEM MEMORY API
pub fn allocate_frame() -> Option<usize> {
    let mut buddy = FRAME_ALLOCATOR.lock();
    let virt_addr = buddy.alloc(0)? as usize;
    let phys_addr = virt_addr - crate::paging::PHYS_OFFSET as usize;
    Some(phys_addr)
}

pub fn clear_frame(phys_addr: usize) {
    let mut buddy = FRAME_ALLOCATOR.lock();
    let virt_addr = phys_addr + crate::paging::PHYS_OFFSET as usize;
    buddy.free(virt_addr, 0);
}

pub fn allocate_zeroed_frame() -> Option<usize> {
    let frame_addr = allocate_frame()?;
    let ptr = (frame_addr + crate::paging::PHYS_OFFSET as usize) as *mut u8;
    unsafe {
        core::ptr::write_bytes(ptr, 0, 4096);
    }
    Some(frame_addr)
}

// THE GRAND BOOTSTRAPPER
pub fn init_physical_memory(memory_map: &'static [crate::boot_info::MemoryMapEntry], bump_alloc: &mut BumpAllocator) {
    crate::serial_println!("Initializing O(1) Buddy Allocator...");

    let mut highest_addr = 0;
    for mem in memory_map {
        if mem.typ == 1 {
            let region_end = (mem.base_addr + mem.length) as usize;
            if region_end > highest_addr {
                highest_addr = region_end;
            }
        }
    }
    // Steal memory for the Bitmap
    let bitmap_size = crate::buddy::calculate_bitmap_size(highest_addr);
    let frames_for_bitmap = (bitmap_size + 4095) / 4096;

    let bitmap_phys_addr = bump_alloc
        .allocate_contiguous_frames(frames_for_bitmap)
        .expect("FATAL: Not enough memory for Buddy Bitmap");

    let bitmap_virt_ptr = (bitmap_phys_addr + crate::paging::PHYS_OFFSET as usize) as *mut u8;
    let bitmap_phys_end = bitmap_phys_addr + (frames_for_bitmap * 4096);

    //  Reserve the bitmap memory dynamically so it doesn't feed itself into the free list!
    unsafe {
        crate::memory::reserve_region(bitmap_phys_addr, bitmap_phys_end, "Buddy Allocator Bitmap");
        core::ptr::write_bytes(bitmap_virt_ptr, 0, bitmap_size);
        FRAME_ALLOCATOR.lock().init(
            bitmap_virt_ptr,
            bitmap_size,
            crate::paging::PHYS_OFFSET as usize,
        );
    }

    // Feed unreserved RAM into the Buddy System
    let mut buddy = FRAME_ALLOCATOR.lock();
    let mut free_frames = 0;

    for mem in memory_map {
        if mem.typ == 1 {
            let region_start = (mem.base_addr as usize + 4095) & !4095;
            let region_end = (mem.base_addr as usize + mem.length as usize) & !4095;

            let mut chunk_start = region_start;
            while chunk_start < region_end {
                if is_reserved(chunk_start, 4096) {
                    chunk_start += 4096;
                    continue;
                }
                // Find end of this contiguous unreserved run
                let mut chunk_end = chunk_start;
                while chunk_end < region_end && !is_reserved(chunk_end, 4096) {
                    chunk_end += 4096;
                    free_frames += 1;
                }
                let virt_start = chunk_start + crate::paging::PHYS_OFFSET as usize;
                let virt_end = chunk_end + crate::paging::PHYS_OFFSET as usize;
                buddy.add_free_region(virt_start, virt_end);
                chunk_start = chunk_end;
            }
        }
    }

    crate::serial_println!(
        "Physical Buddy Allocator Live! Managing {} MB of Free RAM",
        (free_frames * 4096) / 1024 / 1024
    );
}

pub fn init_heap(_p4_table: &mut x86_64::structures::paging::PageTable) {
    crate::serial_println!("Dynamic heap initialized. Waiting for heap allocation");
}
