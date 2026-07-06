// src/mm/paging.rs

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{Page, PageTable, PageTableFlags, PhysFrame};

/// The physical-to-virtual offset used for the higher-half kernel mapping.
///
/// All physical addresses are accessible at `phys_addr + PHYS_OFFSET` in the
/// kernel's virtual address space.  The value `0xFFFF_8000_0000_0000` places
/// the mapping in the upper canonical half of the 64-bit address space, safely
/// above any user-space addresses and meeting the requirements of the x86_64
/// higher-half kernel design.
pub const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000; // Now the kernel in in the higher half of memory

/// Returns a mutable reference to the active level-4 (PML4) page table.
///
/// Reads the current value of `CR3` to obtain the physical address of the PML4
/// table, then adds [`PHYS_OFFSET`] to derive its virtual alias and returns a
/// mutable reference.
///
/// # Safety
/// The returned reference is `'static` and mutable, so the caller must ensure
/// that no other reference to the same page table exists simultaneously.
pub fn active_level_4_table() -> &'static mut PageTable {
    let cr3 = Cr3::read();

    let physical_address = cr3.0.start_address();

    let virtual_addres = physical_address.as_u64() + PHYS_OFFSET;

    let page_table_ptr = virtual_addres as *mut PageTable;

    return unsafe { &mut *page_table_ptr };
}

/// Maps a virtual `page` to a physical `frame` with the given `flags` in `p4_table`.
///
/// # How it works
///
/// Walks the four-level page-table hierarchy (PML4 → PDPT → PD → PT),
/// allocating intermediate page-table pages on demand via
/// [`crate::mm::memory::allocate_zeroed_frame`] when a level is not yet
/// present.  At each intermediate level the entry is given `PRESENT | WRITABLE
/// | USER_ACCESSIBLE` flags (the leaf entry uses the caller-supplied `flags`).
///
/// After writing the level-1 entry the TLB entry for `page` is flushed with
/// `invlpg` so the CPU immediately sees the new mapping.
///
/// # Errors
/// - [`MapToError::FrameAllocationFailed`] — ran out of physical frames for
///   intermediate page-table pages.
/// - [`MapToError::PageAlreadyMapped`] — the target level-1 entry is already
///   non-empty (the caller's logical error).
pub fn map_to(
    page: Page,
    frame: PhysFrame,
    flags: PageTableFlags,
    p4_table: &mut PageTable,
) -> Result<(), MapToError<x86_64::structures::paging::Size4KiB>> {
    let p4_entry = &mut p4_table[page.p4_index()];
    if p4_entry.is_unused() {
        let frame_addr = crate::mm::memory::allocate_zeroed_frame();

        match frame_addr {
            Some(frame_addr) => {
                let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);

                let table_flags = PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE;
                p4_entry.set_addr(phy_addr, table_flags);
            }
            None => {
                return Err(MapToError::FrameAllocationFailed);
            }
        }
    } else {
        // Ensure that is has the correct flags
        let required_flags = flags & (PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);

        p4_entry.set_flags(p4_entry.flags() | required_flags);
    }

    let p4_addr = p4_entry.addr();
    let virtual_addr = p4_addr.as_u64() + PHYS_OFFSET;

    // Cast the pointer to a raw pointer
    let p4_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p3_table = unsafe { &mut *p4_table_ptr };

    let p3_entry = &mut p3_table[page.p3_index()];
    if p3_entry.is_unused() {
        let frame_addr = crate::mm::memory::allocate_zeroed_frame();
        match frame_addr {
            Some(frame_addr) => {
                let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);
                let table_flags = PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE;
                p3_entry.set_addr(phy_addr, table_flags);
            }
            None => {
                return Err(MapToError::FrameAllocationFailed);
            }
        }
    } else {
        // Ensure that is has the correct flags
        let required_flags = flags
            & (PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::USER_ACCESSIBLE);
        p3_entry.set_flags(p3_entry.flags() | required_flags);
    }

    let p3_addr = p3_entry.addr();
    let virtual_addr = p3_addr.as_u64() + PHYS_OFFSET;

    // Cast the pointer to a raw pointer
    let p3_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p2_table = unsafe { &mut *p3_table_ptr };

    let p2_entry = &mut p2_table[page.p2_index()];
    if p2_entry.is_unused() {
        let frame_addr = crate::mm::memory::allocate_zeroed_frame();
        match frame_addr {
            Some(frame_addr) => {
                let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);
                let table_flags = PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::USER_ACCESSIBLE;
                p2_entry.set_addr(phy_addr, table_flags);
            }
            None => {
                return Err(MapToError::FrameAllocationFailed);
            }
        }
    } else {
        // Ensure that is has the correct flags
        let required_flags = flags & (PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE);

        p2_entry.set_flags(p2_entry.flags() | required_flags);
    }

    let p2_addr = p2_entry.addr();
    let virtual_addr = p2_addr.as_u64() + PHYS_OFFSET;

    // Cast the pointer to a raw pointer
    let p2_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p1_table = unsafe { &mut *p2_table_ptr };

    let p1_entry = &mut p1_table[page.p1_index()];

    // If the entry is already mapped, then we do not need to map it again
    if !p1_entry.is_unused() {
        return Err(MapToError::PageAlreadyMapped(frame));
    }

    p1_entry.set_addr(frame.start_address(), flags | PageTableFlags::PRESENT);

    // Critical section: we need to flush the TLB so that the changes we made
    // are visible to the processor
    x86_64::instructions::tlb::flush(page.start_address());
    Ok(())
}

/// Unmaps `page` from `p4_table`, returning the physical frame it was backed by.
///
/// Walks the four-level page-table tree to find the level-1 entry for `page`.
/// If any level is not present, returns `None` immediately.  When the leaf
/// entry is found, it is zeroed (`set_unused`) and the TLB entry is flushed,
/// then the physical frame address is returned so the caller can optionally
/// free it.
///
/// The physical frame is **not** returned to the frame allocator — the caller
/// is responsible for that if desired.
pub fn unmap(page: Page, p4_table: &mut PageTable) -> Option<PhysFrame> {
    let p4_entry = &mut p4_table[page.p4_index()];
    if !p4_entry
        .flags()
        .contains(x86_64::structures::paging::PageTableFlags::PRESENT)
    {
        return None;
    }
    let p3_table_ptr = (p4_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p3_table = unsafe { &mut *p3_table_ptr };

    let p3_entry = &mut p3_table[page.p3_index()];
    if !p3_entry
        .flags()
        .contains(x86_64::structures::paging::PageTableFlags::PRESENT)
    {
        return None;
    }
    let p2_table_ptr = (p3_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p2_table = unsafe { &mut *p2_table_ptr };

    let p2_entry = &mut p2_table[page.p2_index()];
    if !p2_entry
        .flags()
        .contains(x86_64::structures::paging::PageTableFlags::PRESENT)
    {
        return None;
    }
    let p1_table_ptr = (p2_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p1_table = unsafe { &mut *p1_table_ptr };

    let p1_entry = &mut p1_table[page.p1_index()];
    if !p1_entry
        .flags()
        .contains(x86_64::structures::paging::PageTableFlags::PRESENT)
    {
        return None;
    }

    let phys_addr = p1_entry.addr();

    //  Physically sever the connection in the page table
    p1_entry.set_unused();

    // Flush the CPU cache for this virtual address
    x86_64::instructions::tlb::flush(page.start_address());

    Some(x86_64::structures::paging::PhysFrame::containing_address(
        phys_addr,
    ))
}

/// Translates a virtual address to its corresponding physical address.
///
/// Performs a software page-table walk identical to the CPU's MMU, traversing
/// all four levels of the hierarchy.  Returns `None` if any level's entry is
/// not present.
///
/// Also handles 2 MiB huge pages at the PD level: if the `HUGE_PAGE` flag is
/// set in the PD entry the function extracts the 21-bit page offset from
/// `virt_addr` and adds it directly to the frame base.
pub fn translate_addr(virt_addr: x86_64::VirtAddr) -> Option<x86_64::PhysAddr> {
    let p4_table = active_level_4_table();

    let p4_index = virt_addr.p4_index();
    let p3_index = virt_addr.p3_index();
    let p2_index = virt_addr.p2_index();
    let p1_index = virt_addr.p1_index();

    let p4_entry = &p4_table[p4_index];
    if p4_entry.is_unused() {
        return None;
    }

    let p3_table_ptr = (p4_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p3_table = unsafe { &mut *p3_table_ptr };

    let p3_entry = &p3_table[p3_index];
    if p3_entry.is_unused() {
        return None;
    }

    let p2_table_ptr = (p3_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p2_table = unsafe { &mut *p2_table_ptr };

    let p2_entry = &p2_table[p2_index];
    if p2_entry.is_unused() {
        return None;
    }

    if p2_entry
        .flags()
        .contains(x86_64::structures::paging::PageTableFlags::HUGE_PAGE)
    {
        let huge_offset = virt_addr.as_u64() & 0x1FFFFF;
        return Some(x86_64::PhysAddr::new(
            p2_entry.addr().as_u64() + huge_offset,
        ));
    }

    let p1_table_ptr = (p2_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p1_table = unsafe { &mut *p1_table_ptr };

    let p1_entry = &p1_table[p1_index];
    if p1_entry.is_unused() {
        return None;
    }
    //  Get the base address of the 4KB physical frame
    let frame_base = p1_entry.addr().as_u64();

    // Extract the 12-bit page offset from the original virtual address
    let page_offset = virt_addr.as_u64() & 0xFFF; // 0xFFF is 12 bits

    // Combine them for the exact byte address!
    Some(x86_64::PhysAddr::new(frame_base + page_offset))
}

/// Creates a minimal Ring-3 sandbox environment and returns `(code_addr, stack_top_addr)`.
///
/// # Steps
///
/// 1. Allocates one physical frame for code and one for the stack using
///    [`crate::mm::memory::allocate_frame`].
/// 2. Maps both frames into the current address space with `USER_ACCESSIBLE |
///    PRESENT | WRITABLE` flags at fixed virtual addresses:
///    - Code at `0x4000_0000` (1 GiB mark).
///    - Stack at `0x8000_0000` (2 GiB mark).
/// 3. Copies a tiny hardcoded loop of x86-64 machine code into the code page:
///    ```asm
///    loop:
///        mov eax, 42   ; syscall number
///        syscall
///        jmp loop
///    ```
/// 4. Returns the code virtual address and the **top** of the stack page
///    (`stack_virt_addr + 4096`), since x86-64 stacks grow downward.
///
/// The caller is expected to use these addresses with
/// [`crate::arch::x86_64::gdt::jump_to_user_space`].
/// Creates a minimal Ring 3 environment and returns (Code_Addr, Stack_Top_Addr)
pub fn setup_user_sandbox() -> (u64, u64) {
    let active_table = crate::mm::paging::active_level_4_table();

    // The user flags required to survive Ring 3 memory accesses
   let code_flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;

    let stack_flags = PageTableFlags::PRESENT 
        | PageTableFlags::WRITABLE 
        | PageTableFlags::USER_ACCESSIBLE 
        | PageTableFlags::NO_EXECUTE;

    let code_virt_addr = x86_64::VirtAddr::new(0x4000_0000); // 1 GB mark
    let stack_virt_addr = x86_64::VirtAddr::new(0x8000_0000); // 2 GB mark

    let code_phys_addr = crate::mm::memory::allocate_frame().expect("OOM");
    let stack_phys_addr = crate::mm::memory::allocate_frame().expect("OOM");

    let code_page =
        Page::<x86_64::structures::paging::Size4KiB>::containing_address(code_virt_addr);
    let code_frame = PhysFrame::<x86_64::structures::paging::Size4KiB>::containing_address(
        x86_64::PhysAddr::new(code_phys_addr as u64),
    );

    let stack_page =
        Page::<x86_64::structures::paging::Size4KiB>::containing_address(stack_virt_addr);
    let stack_frame = PhysFrame::<x86_64::structures::paging::Size4KiB>::containing_address(
        x86_64::PhysAddr::new(stack_phys_addr as u64),
    );

    crate::mm::paging::map_to(code_page, code_frame, code_flags, active_table).expect("OOM");
    crate::mm::paging::map_to(stack_page, stack_frame, stack_flags, active_table).expect("OOM");

    // This assembly translates to:
    // loop:
    //   mov eax, 42    (0xB8, 0x2A, 0x00, 0x00, 0x00)
    //   syscall        (0x0F, 0x05)
    //   jmp loop       (0xEB, 0xF7)
    let machine_code: [u8; 9] = [0xB8, 0x2A, 0x00, 0x00, 0x00, 0x0F, 0x05, 0xEB, 0xF7];

    unsafe {
        core::ptr::copy_nonoverlapping(
            machine_code.as_ptr(),
            code_virt_addr.as_mut_ptr::<u8>(),
            machine_code.len(),
        );
    }

    // Note: Stacks grow downwards, so return the *top* of the stack page!
    (code_virt_addr.as_u64(), stack_virt_addr.as_u64() + 4096)
}

// In src/mm/paging.rs

/// Dynamically expands the PHYS_OFFSET direct mapping using 2 MiB Huge Pages
/// to cover all physical RAM up to `highest_phys_addr`.
pub fn map_all_physical_memory(
    highest_phys_addr: u64,
    p4_table: &mut x86_64::structures::paging::PageTable,
    bump_alloc: &mut crate::mm::memory::BumpAllocator,
) {
    use x86_64::structures::paging::{PageTable, PageTableFlags};

    // We already mapped 0 -> 1 GiB in boot.asm. Start mapping at 1 GiB (0x4000_0000).
    let start_phys: u64 = 0x4000_0000;

    let align_mask = (1 << 21) - 1; // 0x1FFFFF
    let end_phys: u64 = (highest_phys_addr + align_mask) & !align_mask;

    let mut current_phys = start_phys;

    while current_phys < end_phys {
        let virt_addr = x86_64::VirtAddr::new(current_phys + PHYS_OFFSET);

        let p4_idx = virt_addr.p4_index();
        let p3_idx = virt_addr.p3_index();
        let p2_idx = virt_addr.p2_index();

        if p4_table[p4_idx].is_unused() {
            let frame = bump_alloc.allocate_contiguous_frames(1).expect("Early OOM");
            unsafe {
                core::ptr::write_bytes((frame as u64 + PHYS_OFFSET) as *mut u8, 0, 4096);
            }
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            p4_table[p4_idx].set_addr(x86_64::PhysAddr::new(frame as u64), flags);
        }

        let p3_table_ptr = (p4_table[p4_idx].addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
        let p3_table = unsafe { &mut *p3_table_ptr };

        if p3_table[p3_idx].is_unused() {
            let frame = bump_alloc.allocate_contiguous_frames(1).expect("Early OOM");
            unsafe {
                core::ptr::write_bytes((frame as u64 + PHYS_OFFSET) as *mut u8, 0, 4096);
            }
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            p3_table[p3_idx].set_addr(x86_64::PhysAddr::new(frame as u64), flags);
        }

        let p2_table_ptr = (p3_table[p3_idx].addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
        let p2_table = unsafe { &mut *p2_table_ptr };

        let leaf_flags =
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE;
        p2_table[p2_idx].set_addr(x86_64::PhysAddr::new(current_phys), leaf_flags);

        current_phys += 1 << 21;
    }

    // Flush TLB so CPU sees expanded mappings
    x86_64::instructions::tlb::flush_all();
}
