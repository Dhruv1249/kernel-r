// src/lib.rs

// Adding no_std to the crate allows it to be used in a no-std environment.
// Since rust's standard libraries depends on dependencies like libc, provided by the os.
// We won't be able to use the standard library in this case.
// That's why disabling it here.
#![no_std]
// Also just learned #! -> for whole crate and only # -> for module directly below it!
#![feature(abi_x86_interrupt)] 
// We think rust starts with main, but in reality its entry point is a _start functions which
// sets up the stacks, heap, backtrace for panics etc but all of it is provided in the stdlib
// so we will override the entry point.
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
pub fn test_runner(tests: &[ ( &str, &dyn Fn()   )]){
    use qemu::QemuExitCode;
    println!("Running {} tests.........",tests.len());
    for test in tests{
        serial_println!("Running {}",test.0);
        test.1();
    }
    exit_qemu(QemuExitCode::Success);
}
extern crate alloc;

// Our imports here.
mod gdt;
mod interrupt;
mod qemu;
mod serial;
mod vga_buffer;
mod boot_info;
mod memory;
mod paging;
mod allocator;
use core:: panic::PanicInfo;

use crate::{memory::ALLOCATOR, qemu::exit_qemu};


fn dump_registers() {
    serial_println!("Dumping registers");
    let rflags = x86_64::registers::rflags::read();
    serial_println!("rflags: {:#x}", rflags.bits());
    let rip = x86_64::registers::control::Cr2::read();
    serial_println!("rip: {:#x}", rip);
    let mut rsp: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
    }
    serial_println!("rsp: {:#x}", rsp);
}



// Defining a panic handler allows us to take care of the error gracefully.
// Again without std, we will have to define a panic handler otherwise it won't compile.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    #[cfg(feature = "test")]
    {
        serial_println!("Test panicked");
        serial_println!("{}", info);
        // exit_qemu(crate::qemu::QemuExitCode::Failure);
    }

    serial_println!("Kernel Panicked!");
    serial_println!("{}", info);
    println!("Kernel Panicked!");
    println!("{}", info);
    dump_registers();
    loop {
        x86_64::instructions::hlt(); // Puts the CPU to sleep until the next interrupt (which won't matter here)
    }
}


// Defining our global heap
#[global_allocator]
static HEAP_ALLOCATOR: crate::allocator::Locked<crate::allocator::LinkedListAllocator> = 
    crate::allocator::Locked::new(crate::allocator::LinkedListAllocator::new());


unsafe extern "C" {
    static stack_bottom: u8;
    static stack_top: u8;
}


unsafe extern "C" {
    static kernel_start: u8;
    static kernel_end: u8;
}

// Using no_mangle to disable name mangling.
// Usually whenever rust compiles it gives each functions its own uniquely generated
// cryptic id to differentiate it from all functions (it helps in overloading).
// But in our case _start is the entry point for program and we always want it to have same
// name.
#[unsafe( no_mangle )]
// extern "C" tells rust to call the functions just like C since bootloader expects
// functions to be called specifically like C like register/stack positions and we want
// stability.
pub extern "C" fn _start(multiboot_info_addr: usize, grub_magic_number: usize) -> ! {

    // Load the GDT and the IDT
    gdt::init();
    interrupt::load_idt();
    
    // Clear the screen
    crate::vga_buffer::WRITER.lock().clear();

    // Phyiscal offset
    let phy_offset = crate::paging::PHYS_OFFSET;

    let stack_var = 0u64;
    let stack_addr = &stack_var as *const _ as u64;
    let k_start = &raw const kernel_start as usize - phy_offset as usize;
    let k_end = &raw const kernel_end as usize - phy_offset as usize;
    let st_top = &raw const stack_top as usize - phy_offset as usize;
    let st_bottom = &raw const stack_bottom as usize - phy_offset as usize;
   
    // Debug prints
    serial_println!("Stack bottom at {:#x}", &raw const stack_bottom as u64);
    serial_println!("Stack top at {:#x}", &raw const stack_top as u64);
    serial_println!("Stack is at: {:#x}", stack_addr);
    serial_println!("Kernel code at: {:#x}", _start as *const () as u64);
    serial_println!("Multiboot info at: {:#x}", multiboot_info_addr);
    serial_println!("GRUB magic number: {:#x}", grub_magic_number);
    serial_println!("Kernel start: {:#x}", k_start);
    serial_println!("Kernel end: {:#x}", k_end);


    let mbi_ptr = multiboot_info_addr as *const u32;

    let mbi = unsafe { &*mbi_ptr };
    serial_println!("Multiboot size: {:?}", mbi);

    let tag_iter = boot_info::TagIterator::new(multiboot_info_addr);

    for tag in tag_iter {
        let tag_header = unsafe { &*tag };

        if tag_header.typ == 6 {
            let mmap_entry = unsafe {
                &*(tag as *const boot_info::MemoryMapTag)
            };

            let num_entries = (mmap_entry.size -16) / mmap_entry.entry_size;


            // First entry starts exactly 16 bytes after the tag tag header
            let first_entry_ptr = (tag as usize + 16 ) as *const boot_info::MemoryMapEntry;

            let entries = unsafe {
                core::slice::from_raw_parts(first_entry_ptr, num_entries as usize)
            };

            let mut allocator = crate::memory::BumpAllocator::init(
                k_end, 
                entries, 
                multiboot_info_addr, 
                multiboot_info_addr+ (*mbi as usize), 
                st_bottom, 
                st_top 
                );


            let bitmap_allocator = crate::memory::BitmapAllocator::init(entries, &mut allocator);
            // Move the allocator to the global lock
            *crate::memory::ALLOCATOR.lock() = Some(bitmap_allocator);

            serial_print!("Allocating frame1: {:#x?}\n", crate::memory::allocate_frame()); 
            serial_print!("Allocating frame2: {:#x?}\n", crate::memory::allocate_frame()); 
            serial_print!("Allocating frame3: {:#x?}\n", crate::memory::allocate_frame()); 

        }
    }

    let p4_table = paging::active_level_4_table();

    // Remove 0x0 identity mapping

    // Set p4 index 0 to 0
    p4_table[0].set_unused();

    // Flush the TLB fully so the CPU completely forgets about the old mapping
    x86_64::instructions::tlb::flush_all();

    serial_println!("Successfully severed the identity mapping");


    // Setup guard page
    // Setup guard page using the VIRTUAL address!
    let virtual_st_bottom = &raw const stack_bottom as u64;
    let guard_page_addr = x86_64::VirtAddr::new(virtual_st_bottom- 4096);
    let guard_page: x86_64::structures::paging::Page = x86_64::structures::paging::Page::containing_address(guard_page_addr);

    
    crate::serial_println!("Mapping guard page");
    if let Some(_page) = crate::paging::unmap(guard_page, p4_table){
        serial_println!("Successfully unmapped the guard page");
    } else {
        serial_println!("Failed to unmap the guard page");
    }

    use x86_64::VirtAddr;
    // Take absure virtual address for testing
    let virt_addr = VirtAddr::new(0x1000_0000_0000);
    let page = x86_64::structures::paging::Page::containing_address(virt_addr);

    //  Ask our bump allocator for a fresh physical frame to back it
    let physical_frame_addr = crate::memory::allocate_frame().unwrap();
    let frame = x86_64::structures::paging::PhysFrame::containing_address(
        x86_64::PhysAddr::new(physical_frame_addr as u64)
    );

    //  Map them together!
    use x86_64::structures::paging::PageTableFlags;
    crate::paging::map_to(page, frame, PageTableFlags::WRITABLE, p4_table);

    // Test the mapping by writing to the VIRTUAL address
    crate::serial_println!("Mapping successful! Writing to virtual address...");
    let page_ptr = virt_addr.as_mut_ptr::<u64>();
    unsafe {
        // Write a recognizable hex value
        *page_ptr = 0xABCDEF;
    }

    crate::serial_println!("Successfully read back: {:#X}", unsafe { *page_ptr });

    // Initalize the heap
    crate::memory::init_heap(p4_table);

    // Give pages to the heap
    unsafe {
        crate::HEAP_ALLOCATOR.lock().init(crate::memory::HEAP_START, crate::memory::HEAP_SIZE);
    }

    crate::serial_println!("Heap initialized");

    // --- TESTING HEAP COALESCING ---
    crate::serial_println!("--- Starting Heap Coalescing Test ---");
    use alloc::vec::Vec;

    // 1. Allocate 3 separate blocks (10,000 bytes each)
    let vec1: Vec<u8> = Vec::with_capacity(10_000_00);
    let vec2: Vec<u8> = Vec::with_capacity(10_000_00);
    let vec3: Vec<u8> = Vec::with_capacity(10_000_00);
    crate::serial_println!("Allocated 3 vectors (10KB each).");

    //  Fragment the heap by dropping them out of order
    drop(vec2);
    crate::serial_println!("Dropped middle vector (Created a hole).");
    
    drop(vec1);
    crate::serial_println!("Dropped first vector (Triggered Merge Right!).");
    
    drop(vec3);
    crate::serial_println!("Dropped third vector (Triggered Merge Left!).");

    //  The Ultimate Test: Ask for lagre amount of bytes. 
    // If coalescing failed, the heap is split into three 10K blocks, 
    // and this will instantly trigger an Out-Of-Memory panic!
    let huge_vec: Vec<u8> = Vec::with_capacity(1024*1023*10);
    crate::serial_println!("SUCCESS! Allocated huge vector of capacity: {}", huge_vec.capacity());
    crate::serial_println!("--- Heap Coalescing Works! ---");

    // crate::vga_buffer::WRITER.lock().clear();

    use alloc::string::String;
    let mut test_string = String::new();
    test_string.push_str("Hello world");
    println!("Test string: {:?}", test_string);
  
    // stack_overflow();
    // #[cfg(feature = "test")]
    // test_main();


    loop{}
}

// fn stack_overflow() {
//     stack_overflow();
// }
//
// fn testing() {
//     assert_eq!(1, 1);
//     serial_println!("testing... ok");
// }
//
// #[cfg(feature = "test")]
// pub fn test_main() {
//     serial_println!("Running tests...");
//     test_runner(&[( "testing",&testing )]);
// }
