// src/paging.rs

use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{self, Page, PageTable, PageTableFlags, PhysFrame};


pub const PHYS_OFFSET: u64 = 0xFFFF_8000_0000_0000; // Now the kernel in in the higher half of memory

pub fn active_level_4_table() -> &'static mut PageTable {
    let cr3 = Cr3::read();

    let physical_address = cr3.0.start_address();

    let virtual_addres = physical_address.as_u64() + PHYS_OFFSET;

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
    let virtual_addr = p4_addr.as_u64() + PHYS_OFFSET;

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
    let virtual_addr = p3_addr.as_u64() + PHYS_OFFSET;

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
    let virtual_addr = p2_addr.as_u64() + PHYS_OFFSET;

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


pub fn unmap(page: Page, p4_table: &mut PageTable) -> Option<PhysFrame> {
    let p4_entry = &mut p4_table[page.p4_index()];
    if !p4_entry.flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
        return None;
    }
    let p3_table_ptr = (p4_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p3_table = unsafe { &mut *p3_table_ptr };

    let p3_entry = &mut p3_table[page.p3_index()];
    if !p3_entry.flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
        return None;
    }
    let p2_table_ptr = (p3_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p2_table = unsafe { &mut *p2_table_ptr };

    let p2_entry = &mut p2_table[page.p2_index()];
    if !p2_entry.flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
        return None;
    }
    let p1_table_ptr = (p2_entry.addr().as_u64() + PHYS_OFFSET) as *mut PageTable;
    let p1_table = unsafe { &mut *p1_table_ptr };

    let p1_entry = &mut p1_table[page.p1_index()];
    if !p1_entry.flags().contains(x86_64::structures::paging::PageTableFlags::PRESENT) {
        return None;
    }

    let phys_addr = p1_entry.addr();
    
    //  Physically sever the connection in the page table
    p1_entry.set_unused();
    
    // Flush the CPU cache for this virtual address
    x86_64::instructions::tlb::flush(page.start_address());
    
    // Clear the frame in the physical bitmap allocator
    crate::memory::clear_frame(phys_addr.as_u64() as usize);
    
    Some(x86_64::structures::paging::PhysFrame::containing_address(phys_addr))
}


pub fn translate_addr(virt_addr: x86_64::VirtAddr) -> Option<x86_64::PhysAddr>{
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

    if p2_entry.flags().contains(x86_64::structures::paging::PageTableFlags::HUGE_PAGE) {
        let huge_offset = virt_addr.as_u64() & 0x1FFFFF;
        return Some(x86_64::PhysAddr::new(p2_entry.addr().as_u64() + huge_offset));
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


