// src/mm/memory.rs

/// The globally-shared physical frame allocator, backed by the buddy system.
///
/// Protected by [`crate::mm::allocator::Locked`] so it can be safely accessed
/// from both normal kernel code and interrupt handlers.  Initialised to an
/// empty placeholder and fully populated during [`init_physical_memory`].
// Start as an empty allocator, initialized fully at boot
pub static FRAME_ALLOCATOR: crate::mm::allocator::Locked<crate::mm::buddy::BuddyAllocator> =
    crate::mm::allocator::Locked::new(crate::mm::buddy::BuddyAllocator::empty());

/// Virtual start address of the kernel heap.
///
/// Chosen to be in the upper half of the 64-bit address space, far away from
/// any identity-mapped physical memory.  Pages in this region are demand-mapped
/// by the page-fault handler in `interrupts::interrupt`.
pub const HEAP_START: usize = 0x_4444_4444_0000;

/// Size of the kernel heap in bytes (10 MiB).
///
/// The heap spans `[HEAP_START, HEAP_START + HEAP_SIZE)`.  Pages are only
/// physically backed when first written (demand paging).
pub const HEAP_SIZE: usize = 0xA00000; // 10 MB

/// A record of one physical memory region that must not be handed to the frame allocator.
///
/// Regions are registered via [`reserve_region`] before the buddy allocator is
/// initialised, so that the boot code, kernel image, stack, VGA buffer, and
/// Multiboot2 structures are never accidentally given out as free frames.
#[derive(Copy, Clone)]
pub struct ReservedRegion {
    pub start: usize,
    pub end: usize,
    pub name: &'static str,
}

/// Maximum number of simultaneously reserved physical memory regions.
const MAX_RESERVED: usize = 32;

/// Legacy global mutable reservation array — kept for backward compatibility.
///
/// New code should use [`RESERVED_TRACKER`] instead, which is protected by a
/// spinlock.
pub static mut RESERVED_REGIONS: [ReservedRegion; MAX_RESERVED] = [ReservedRegion {
    start: 0,
    end: 0,
    name: "",
}; MAX_RESERVED];

/// Legacy count of reserved regions — kept for backward compatibility.
pub static mut RESERVED_COUNT: usize = 0;

use spin::Mutex;

/// Internal state of the region tracker, holding both the array and the count.
///
/// Bundled together so the whole thing can be protected by a single spinlock
/// with no race between reading `count` and reading `regions`.
pub struct RegionTracker {
    regions: [ReservedRegion; MAX_RESERVED],
    count: usize,
}

/// Spinlock-protected registry of reserved physical memory regions.
///
/// Any physical range that must not be given to the frame allocator is
/// registered here before [`init_physical_memory`] is called.  The buddy
/// allocator's feeding loop checks [`is_reserved`] for every frame before
/// adding it to the free lists.
// Safely wrapped in a Spinlock
pub static RESERVED_TRACKER: Mutex<RegionTracker> = Mutex::new(RegionTracker {
    regions: [ReservedRegion {
        start: 0,
        end: 0,
        name: "",
    }; MAX_RESERVED],
    count: 0,
});

/// Registers `[start, end)` as a reserved physical memory region.
///
/// Acquires the [`RESERVED_TRACKER`] spinlock, appends the new region, and
/// prints a diagnostic line on the serial console.  Panics if the reservation
/// table is full (more than [`MAX_RESERVED`] regions).
pub fn reserve_region(start: usize, end: usize, name: &'static str) {
    let mut tracker = RESERVED_TRACKER.lock();
    if tracker.count < MAX_RESERVED {
        let count = tracker.count;
        tracker.regions[count] = ReservedRegion { start, end, name };
        tracker.count += 1;
        crate::serial_println!("Reserved: {} [{:#x} - {:#x}]", name, start, end);
    } else {
        panic!("Out of reserved region slots!");
    }
}

/// Returns `true` if the physical range `[phys_addr, phys_addr + size)` overlaps
/// any registered reserved region.
///
/// Uses the standard overlap test: two ranges `[a, b)` and `[c, d)` overlap
/// iff `a < d && b > c`.  Iterates over all registered regions under the
/// [`RESERVED_TRACKER`] lock.
pub fn is_reserved(phys_addr: usize, size: usize) -> bool {
    let end_addr = phys_addr + size;
    let tracker = RESERVED_TRACKER.lock();

    for i in 0..tracker.count {
        let region = &tracker.regions[i];
        // Check for overlap
        if phys_addr < region.end && end_addr > region.start {
            return true;
        }
    }
    false
}

/// A simple bump allocator used only during early-boot to bootstrap the buddy system.
///
/// The bump allocator scans the Multiboot2 memory map for available usable RAM
/// and hands out contiguous page-aligned frame ranges.  It does **not** support
/// deallocation — once a region is bumped past, it is gone.
///
/// It is used exactly once: to allocate the memory needed for the buddy
/// allocator's bitmap.  After that, the buddy system takes over all frame
/// allocation.
// BUMP ALLOCATOR (For bootstrapping only)
pub struct BumpAllocator {
    next_free_frame: usize,
    memory_map: &'static [crate::boot::boot_info::MemoryMapEntry],
}

impl BumpAllocator {
    /// Creates a new `BumpAllocator` starting immediately after the kernel image.
    ///
    /// `kernel_end` is the physical address of the first byte past the loaded
    /// kernel binary (including BSS).  The bump pointer is aligned up to the
    /// next 4 KiB boundary so all allocations are page-aligned.
    pub fn init(
        kernel_end: usize,
        memory_map: &'static [crate::boot::boot_info::MemoryMapEntry],
    ) -> Self {
        let aligned_addr = (kernel_end + 4095) & !(4096 - 1);
        BumpAllocator {
            next_free_frame: aligned_addr,
            memory_map,
        }
    }

    /// Allocates `count` physically contiguous, page-aligned, unreserved frames.
    ///
    /// Iterates over usable (`typ == 1`) memory map entries and searches for a
    /// run of `count` consecutive unreserved 4 KiB frames.  When a reserved
    /// frame is encountered the search position jumps past it and restarts
    /// (inner `continue 'search` loop).
    ///
    /// Returns `Some(phys_addr)` of the first frame in the run, or `None` if
    /// no suitable contiguous range exists.
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

/// Allocates one 4 KiB physical frame from the buddy allocator.
///
/// Acquires the [`FRAME_ALLOCATOR`] lock, requests an order-0 block, and
/// converts the returned virtual address back to a physical address by
/// subtracting `PHYS_OFFSET`.  Returns `None` if the allocator has no free
/// frames.
// SYSTEM MEMORY API
pub fn allocate_frame() -> Option<usize> {
    let mut buddy = FRAME_ALLOCATOR.lock();
    let virt_addr = buddy.alloc(0)? as usize;
    let phys_addr = virt_addr - crate::mm::paging::PHYS_OFFSET as usize;
    Some(phys_addr)
}

/// Returns a 4 KiB physical frame to the buddy allocator.
///
/// Converts the physical address to a virtual address (adding `PHYS_OFFSET`)
/// and calls `buddy.free(virt_addr, 0)`.  The buddy system may merge the freed
/// frame with its buddy if the buddy is also free.
pub fn clear_frame(phys_addr: usize) {
    let mut buddy = FRAME_ALLOCATOR.lock();
    let virt_addr = phys_addr + crate::mm::paging::PHYS_OFFSET as usize;
    buddy.free(virt_addr, 0);
}

/// Allocates one 4 KiB physical frame and zeroes its contents.
///
/// First calls [`allocate_frame`], then writes zero bytes to the entire frame
/// via its virtual alias.  Zeroing is required for page-table pages to ensure
/// all entries start as "not present".
pub fn allocate_zeroed_frame() -> Option<usize> {
    let frame_addr = allocate_frame()?;
    let ptr = (frame_addr + crate::mm::paging::PHYS_OFFSET as usize) as *mut u8;
    unsafe {
        core::ptr::write_bytes(ptr, 0, 4096);
    }
    Some(frame_addr)
}

/// Bootstraps the buddy allocator from the Multiboot2 memory map.
///
/// # Steps
///
/// 1. **Find the highest usable physical address** by scanning all `typ == 1`
///    memory map entries.
/// 2. **Allocate the bitmap** using the bootstrap `BumpAllocator`.  The bitmap
///    size is computed by [`crate::mm::buddy::calculate_bitmap_size`].
/// 3. **Zero the bitmap** and register the region as reserved so it is not
///    recycled into the free list.
/// 4. **Initialise [`FRAME_ALLOCATOR`]** with the bitmap pointer, size, and
///    the virtual-offset base (`PHYS_OFFSET`).
/// 5. **Feed unreserved RAM** into the buddy system via
///    [`crate::mm::buddy::BuddyAllocator::add_free_region`], skipping any frame
///    that overlaps a reserved region.
///
/// Prints a summary of free RAM on the serial console when complete.
// THE GRAND BOOTSTRAPPER
pub fn init_physical_memory(
    memory_map: &'static [crate::boot::boot_info::MemoryMapEntry],
    bump_alloc: &mut BumpAllocator,
) {
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
    let bitmap_size = crate::mm::buddy::calculate_bitmap_size(highest_addr);
    let frames_for_bitmap = (bitmap_size + 4095) / 4096;

    let bitmap_phys_addr = bump_alloc
        .allocate_contiguous_frames(frames_for_bitmap)
        .expect("FATAL: Not enough memory for Buddy Bitmap");

    let bitmap_virt_ptr = (bitmap_phys_addr + crate::mm::paging::PHYS_OFFSET as usize) as *mut u8;
    let bitmap_phys_end = bitmap_phys_addr + (frames_for_bitmap * 4096);

    //  Reserve the bitmap memory dynamically so it doesn't feed itself into the free list!
    unsafe {
        crate::mm::memory::reserve_region(bitmap_phys_addr, bitmap_phys_end, "Buddy Allocator Bitmap");
        core::ptr::write_bytes(bitmap_virt_ptr, 0, bitmap_size);
        FRAME_ALLOCATOR.lock().init(
            bitmap_virt_ptr,
            bitmap_size,
            crate::mm::paging::PHYS_OFFSET as usize,
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
                let virt_start = chunk_start + crate::mm::paging::PHYS_OFFSET as usize;
                let virt_end = chunk_end + crate::mm::paging::PHYS_OFFSET as usize;
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

/// Initialises the kernel heap virtual-address window.
///
/// Currently a no-op: pages are demand-mapped by the page-fault handler in
/// `interrupts::interrupt` when first accessed.  The function signature accepts
/// the active page-table pointer for future use (e.g. pre-mapping a guard page
/// or wiring up huge pages).
pub fn init_heap(_p4_table: &mut x86_64::structures::paging::PageTable) {
    crate::serial_println!("Dynamic heap initialized. Waiting for heap allocation");
}
