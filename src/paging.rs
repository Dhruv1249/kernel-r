// src/paging.rs

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{Page, PageTable, PageTableFlags, PhysFrame};

pub fn active_level_4_table() -> &'static mut PageTable {
    let cr3 = Cr3::read();

    let physical_address = cr3.0.start_address();

    let virtual_addres = physical_address.as_u64();

    let page_table_ptr = virtual_addres as *mut PageTable;

    return unsafe { &mut *page_table_ptr };
}

pub fn map_to(page: Page, frame: PhysFrame, flags: PageTableFlags, p4_table: &mut PageTable) {
    let p4_entry = &mut p4_table[page.p4_index()];
    if p4_entry.is_unused() {
        let frame_addr = crate::memory::allocate_zeroed_frame().expect("Out of physical memory");

        let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);

        let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        p4_entry.set_addr(phy_addr, table_flags);
    }

    let p4_addr = p4_entry.addr();
    let virtual_addr = p4_addr.as_u64();

    // Cast the pointer to a raw pointer
    let p4_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p3_table = unsafe { &mut *p4_table_ptr };

    let p3_entry = &mut p3_table[page.p3_index()];
    if p3_entry.is_unused() {
        let frame_addr = crate::memory::allocate_zeroed_frame().expect("Out of physical memory");
        let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);
        let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        p3_entry.set_addr(phy_addr, table_flags);
    }

    let p3_addr = p3_entry.addr();
    let virtual_addr = p3_addr.as_u64();

    // Cast the pointer to a raw pointer
    let p3_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p2_table = unsafe { &mut *p3_table_ptr };

    let p2_entry = &mut p2_table[page.p2_index()];
    if p2_entry.is_unused() {
        let frame_addr = crate::memory::allocate_zeroed_frame().expect("Out of physical memory");
        let phy_addr = x86_64::PhysAddr::new(frame_addr as u64);
        let table_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        p2_entry.set_addr(phy_addr, table_flags);
    }

    let p2_addr = p2_entry.addr();
    let virtual_addr = p2_addr.as_u64();

    // Cast the pointer to a raw pointer
    let p2_table_ptr = virtual_addr as *mut PageTable;

    // Get a mutable reference to the page table
    let p1_table = unsafe { &mut *p2_table_ptr };

    let p1_entry = &mut p1_table[page.p1_index()];

    // If the entry is already mapped, then we do not need to map it again
    assert!(
        p1_entry.is_unused(),
        "Virtual address {:#x} is already mapped",
        virtual_addr
    );
    p1_entry.set_addr(frame.start_address(), flags | PageTableFlags::PRESENT);

    // Critical section: we need to flush the TLB so that the changes we made
    // are visible to the processor
    x86_64::instructions::tlb::flush(page.start_address());
}
