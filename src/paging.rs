// src/paging.rs

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{Page, PageTable, PageTableFlags, PhysFrame};

pub const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000; // Now the kernel in in the higher half of memory

pub fn active_level_4_table() -> &'static mut PageTable {
    let cr3 = Cr3::read();

    let physical_address = cr3.0.start_address();

    let virtual_addres = physical_address.as_u64() + PHYS_OFFSET;

    let page_table_ptr = virtual_addres as *mut PageTable;

    return unsafe { &mut *page_table_ptr };
}

pub fn map_to(
    page: Page,
    frame: PhysFrame,
    flags: PageTableFlags,
    p4_table: &mut PageTable,
) -> Result<(), MapToError<x86_64::structures::paging::Size4KiB>> {
    let p4_entry = &mut p4_table[page.p4_index()];
    if p4_entry.is_unused() {
        let frame_addr = crate::memory::allocate_zeroed_frame();

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
        let required_flags = flags & (PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE);

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
        let frame_addr = crate::memory::allocate_zeroed_frame();
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
        let frame_addr = crate::memory::allocate_zeroed_frame();
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
        let required_flags = flags & (PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE);

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

/// Creates a minimal Ring 3 environment and returns (Code_Addr, Stack_Top_Addr)
pub fn setup_user_sandbox() -> (u64, u64) {
    let active_table = crate::paging::active_level_4_table();

    // The user flags required to survive Ring 3 memory accesses
    let user_flags =
        PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

    let code_virt_addr = x86_64::VirtAddr::new(0x4000_0000); // 1 GB mark
    let stack_virt_addr = x86_64::VirtAddr::new(0x8000_0000); // 2 GB mark

    let code_phys_addr = crate::memory::allocate_frame().expect("OOM");
    let stack_phys_addr = crate::memory::allocate_frame().expect("OOM");

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

    crate::paging::map_to(code_page, code_frame, user_flags, active_table).expect("OOM");
    crate::paging::map_to(stack_page, stack_frame, user_flags, active_table).expect("OOM");

    let machine_code: [u8; 2] = [0xEB, 0xFE]; // jmp $ (infinite loop)

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
