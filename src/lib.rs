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


// Our imports here.
mod gdt;
mod interrupt;
mod qemu;
mod serial;
mod vga_buffer;
mod boot_info;
mod memory;
mod paging;
use core:: panic::PanicInfo;

use crate::qemu::exit_qemu;





// Defining a panic handler allows us to take care of the error gracefully.
// Again without std, we will have to define a panic handler otherwise it won't compile.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    #[cfg(feature = "test")]
    {
        serial_println!("Test panicked");
        serial_println!("{}", info);
        exit_qemu(crate::qemu::QemuExitCode::Failure);
    }

    println!("{}", info);
    loop {}
}


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
    let stack_var = 0u64;
    let stack_addr = &stack_var as *const _ as u64;
    let k_start = &raw const kernel_start as usize;
    let k_end = &raw const kernel_end as usize;
   
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

            let allocator = crate::memory::BumpAllocator::init(k_end, entries);


            // Move the allocator to the global lock
            *crate::memory::ALLOCATOR.lock() = Some(allocator);

            crate::serial_println!("Allocating frame 1: {:#x?}", crate::memory::allocate_frame());
            crate::serial_println!("Allocating frame 2: {:#x?}", crate::memory::allocate_frame());
            crate::serial_println!("Allocating frame 3: {:#x?}", crate::memory::allocate_frame());
            use x86_64::registers::control::Cr3;
            let cr3 = Cr3::read();
            crate::serial_print!("cr3: {:#x?}", cr3);


            for entry in entries {
                serial_println!(
                    "Base: {:#010x}, Length: {:#010x}, Type: {}", 
                    entry.base_addr, 
                    entry.length, 
                    entry.typ
                )
            }

        }
    }

    let p4_table = paging::active_level_4_table();

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


    // Clear the screen
    vga_buffer::clear_screen();
    // Load the GDT and the IDT
    gdt::init();
    interrupt::load_idt();


    println!("Hello world");
  
    // stack_overflow();
    // println!("after stack overflow");
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
