// src/lib.rs

// Adding no_std to the crate allows it to be used in a no-std environment.
// Since rust's standard libraries depends on dependencies like libc, provided by the os.
// We won't be able to use the standard library in this case.
// That's why disabling it here.
#![no_std]
#![allow(dead_code)]
#![allow(function_casts_as_integer)]
// Also just learned #! -> for whole crate and only # -> for module directly below it!
#![feature(abi_x86_interrupt)]
// We think rust starts with main, but in reality its entry point is a _start functions which
// sets up the stacks, heap, backtrace for panics etc but all of it is provided in the stdlib
// so we will override the entry point.
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
pub fn test_runner(tests: &[(&str, &dyn Fn())]) {
    use drivers::qemu::QemuExitCode;
    println!("Running {} tests.........", tests.len());
    for test in tests {
        serial_println!("Running {}", test.0);
        test.1();
    }
    exit_qemu(QemuExitCode::Success);
}
extern crate alloc;

// Our imports here — now organised into subsystem modules.
pub mod arch;
pub mod boot;
pub mod drivers;
pub mod interrupts;
pub mod mm;
pub mod process;
pub mod sync;
use core::panic::PanicInfo;

use x86_64::structures::paging::Size4KiB;

use crate::{boot::boot_info::TagHeader, drivers::qemu::exit_qemu};

fn dump_registers() {
    serial_println!("Dumping registers");
    let rflags = x86_64::registers::rflags::read();
    serial_println!("rflags: {:#x}", rflags.bits());
    let rip = x86_64::registers::control::Cr2::read();
    serial_println!("Faulting virtual addr: {:#x}", rip);
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
    // #[cfg(feature = "test")]
    // {
    //     serial_println!("Test panicked");
    //     serial_println!("{}", info);
    //     // exit_qemu(crate::drivers::qemu::QemuExitCode::Failure);
    // }

    serial_println!("Kernel Panicked!");
    serial_println!("{}", info);
    println!("Kernel Panicked!");
    println!("{}", info);
    dump_registers();
    loop {
        x86_64::instructions::hlt(); // Puts the CPU to sleep until the next interrupt 
    }
}

pub unsafe fn validate_checksum(start_ptr: *const u8, length: usize) -> bool {
    let mut checksum: u8 = 0;
    for i in 0..length {
        let val = unsafe { core::ptr::read_unaligned(start_ptr.add(i)) };
        checksum = checksum.wrapping_add(val);
    }
    checksum == 0
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
#[unsafe(no_mangle)]
// extern "C" tells rust to call the functions just like C since bootloader expects
// functions to be called specifically like C like register/stack positions and we want
// stability.
pub extern "C" fn _start(multiboot_info_addr: usize, grub_magic_number: usize) -> ! {
    // Load the GDT and the IDT
    crate::arch::x86_64::gdt::init();
    crate::interrupts::interrupt::load_idt();

    // Clear the screen
    crate::drivers::vga_buffer::WRITER.lock().clear();

    // Phyiscal offset
    let phy_offset = crate::mm::paging::PHYS_OFFSET;

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

    crate::mm::memory::reserve_region(0x0, 0x1000, "IVT / BIOS Data");
    crate::mm::memory::reserve_region(0xA0000, 0x100000, "VGA / Legacy Area");

    crate::mm::memory::reserve_region(0x100000, k_end, "Kernel Image + Boot Section");

    crate::mm::memory::reserve_region(
        multiboot_info_addr,
        multiboot_info_addr + (*mbi as usize),
        "Multiboot Info",
    );
    crate::mm::memory::reserve_region(st_bottom - 4096, st_top, "Kernel Stack + Guard Page");

    serial_println!("Multiboot size: {:?}", mbi);

    let tag_iter = crate::boot::boot_info::TagIterator::new(multiboot_info_addr);
    let mut tag_option: Option<*const TagHeader> = None;

    for tags in tag_iter {
        let tag_header = unsafe { &*tags };

        if tag_header.typ == 6 {
            tag_option = Some(tags);
        }
    }

    let tag = tag_option.unwrap();

    let mmap_entry = unsafe { &*(tag as *const crate::boot::boot_info::MemoryMapTag) };

    // Assert that the memory map is valid
    assert!(mmap_entry.entry_size > 0);
    assert!(mmap_entry.size >= 16);
    assert_eq!((mmap_entry.size - 16) % mmap_entry.entry_size, 0);

    let num_entries = (mmap_entry.size - 16) / mmap_entry.entry_size;

    // First entry starts exactly 16 bytes after the tag tag header
    let first_entry_ptr = (tag as usize + 16) as *const crate::boot::boot_info::MemoryMapEntry;

    let entries = unsafe { core::slice::from_raw_parts(first_entry_ptr, num_entries as usize) };

    let mut allocator = crate::mm::memory::BumpAllocator::init(k_end, entries);

    let mut max_phys_addr = 0u64;
    for entry in entries {
        if entry.typ == 1 {
            // 1 = Usable RAM
            let region_end = entry.base_addr + entry.length;
            if region_end > max_phys_addr {
                max_phys_addr = region_end;
            }
        }
    }

    crate::serial_println!(
        "Detected highest physical RAM address: {:#X}",
        max_phys_addr
    );

    crate::mm::paging::map_all_physical_memory(
        max_phys_addr,
        &mut crate::mm::paging::active_level_4_table(),
        &mut allocator,
    );

    crate::mm::memory::reserve_region(k_end, allocator.current_offset(), "Direct Map Page Tables");

    // Bootstrap the Buddy Allocator!
    crate::mm::memory::init_physical_memory(max_phys_addr as usize, entries, &mut allocator);

    unsafe {
        crate::arch::x86_64::cpu::enable_nx_bit();
    }

    serial_print!(
        "Allocating frame1: {:#x?}\n",
        crate::mm::memory::allocate_frame()
    );
    serial_print!(
        "Allocating frame2: {:#x?}\n",
        crate::mm::memory::allocate_frame()
    );
    serial_print!(
        "Allocating frame3: {:#x?}\n",
        crate::mm::memory::allocate_frame()
    );

    // ---  FIND THE MADT PHYSICAL ADDRESS ---
    let mut madt_phys_addr: Option<u64> = None;

    for tag in crate::boot::boot_info::TagIterator::new(multiboot_info_addr) {
        let tag_header = unsafe { &*tag };

        // ACPI Old RSDP Version 1 (32-bit)
        if tag_header.typ == 14 {
            crate::serial_println!("ACPI 1.0 RSDP Found at {:#x}", tag as usize);
            let rsdp = unsafe { &*(tag as *const crate::boot::boot_info::AcpiV1Tag) };
            let rsdt_addr =
                unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(rsdp.rsdt_address)) };
            let rsdt_virt_addr =
                x86_64::VirtAddr::new(rsdt_addr as u64 + crate::mm::paging::PHYS_OFFSET);
            let sdt_header =
                unsafe { &*(rsdt_virt_addr.as_ptr::<crate::boot::boot_info::SdtHeader>()) };

            if unsafe {
                !crate::validate_checksum(
                    sdt_header as *const _ as *const u8,
                    sdt_header.length as usize,
                )
            } {
                panic!("RSDT Checksum failed!");
            }

            let num_entries = (sdt_header.length as usize
                - core::mem::size_of::<crate::boot::boot_info::SdtHeader>())
                / 4;
            let start_ptr = (rsdt_virt_addr.as_u64()
                + core::mem::size_of::<crate::boot::boot_info::SdtHeader>() as u64)
                as *const u32;

            for i in 0..num_entries {
                let entry = unsafe { core::ptr::read_unaligned(start_ptr.add(i)) };
                let entry_virt_addr =
                    x86_64::VirtAddr::new(entry as u64 + crate::mm::paging::PHYS_OFFSET);
                let entry_header =
                    unsafe { &*(entry_virt_addr.as_ptr::<crate::boot::boot_info::SdtHeader>()) };
                let sdt_signature = unsafe {
                    core::ptr::read_unaligned(core::ptr::addr_of!(entry_header.signature))
                };

                if sdt_signature == [b'A', b'P', b'I', b'C'] {
                    madt_phys_addr = Some(entry as u64);
                    break;
                }
            }
        }
        // ACPI New RSDP Version 2 (64-bit)
        else if tag_header.typ == 15 {
            crate::serial_println!("ACPI 2.0 RSDP Found at {:#x}", tag as usize);
            let rsdpv2 = unsafe { &*(tag as *const crate::boot::boot_info::AcpiV2Tag) };
            let xsdt_addr =
                unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(rsdpv2.xsdt_address)) };
            let xsdt_virt_addr =
                x86_64::VirtAddr::new(xsdt_addr as u64 + crate::mm::paging::PHYS_OFFSET);
            let sdt_header =
                unsafe { &*(xsdt_virt_addr.as_ptr::<crate::boot::boot_info::SdtHeader>()) };

            if unsafe {
                !crate::validate_checksum(
                    sdt_header as *const _ as *const u8,
                    sdt_header.length as usize,
                )
            } {
                panic!("XSDT Checksum failed!");
            }

            let num_entries = (sdt_header.length as usize
                - core::mem::size_of::<crate::boot::boot_info::SdtHeader>())
                / 8;
            let start_ptr = (xsdt_virt_addr.as_u64()
                + core::mem::size_of::<crate::boot::boot_info::SdtHeader>() as u64)
                as *const u64;

            for i in 0..num_entries {
                let entry = unsafe { core::ptr::read_unaligned(start_ptr.add(i)) };
                let entry_virt_addr = x86_64::VirtAddr::new(entry + crate::mm::paging::PHYS_OFFSET);
                let entry_header =
                    unsafe { &*(entry_virt_addr.as_ptr::<crate::boot::boot_info::SdtHeader>()) };
                let sdt_signature = unsafe {
                    core::ptr::read_unaligned(core::ptr::addr_of!(entry_header.signature))
                };

                if sdt_signature == [b'A', b'P', b'I', b'C'] {
                    madt_phys_addr = Some(entry);
                    break;
                }
            }
        }
    }

    // --- PARSE MADT & MAP HARDWARE ---
    if let Some(madt_phys) = madt_phys_addr {
        crate::serial_println!("Found MADT (APIC) at physical address: {:#x}", madt_phys);

        let madt_virt = x86_64::VirtAddr::new(madt_phys + crate::mm::paging::PHYS_OFFSET);
        let apic_header = unsafe { &*(madt_virt.as_ptr::<crate::boot::boot_info::Madt>()) };
        let active_table = crate::mm::paging::active_level_4_table();

        // Standard MMIO Flags
        let mmio_flags = x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::NO_CACHE
            | x86_64::structures::paging::PageTableFlags::WRITE_THROUGH;

        // --- LOCAL APIC ---
        let local_apic_addr = unsafe {
            core::ptr::read_unaligned(core::ptr::addr_of!(apic_header.local_apic_address))
        };
        crate::serial_println!("Local APIC Address: {:#x}", local_apic_addr);

        let lapic_phys = x86_64::PhysAddr::new(local_apic_addr as u64);
        let lapic_virt =
            x86_64::VirtAddr::new(local_apic_addr as u64 + crate::mm::paging::PHYS_OFFSET);

        crate::mm::paging::map_to(
            x86_64::structures::paging::Page::<Size4KiB>::containing_address(lapic_virt),
            x86_64::structures::paging::PhysFrame::<Size4KiB>::containing_address(lapic_phys),
            mmio_flags,
            active_table,
        )
        .expect("Failed to map Local APIC");

        let local_apic = unsafe { crate::interrupts::apic::LocalApic::new(lapic_virt) };
        unsafe {
            local_apic.init();
        }
        *crate::interrupts::apic::LOCAL_APIC.lock() = Some(local_apic);

        // --- I/O APIC ---
        let total_table_length =
            unsafe { core::ptr::read_unaligned(core::ptr::addr_of!(apic_header.header.length)) }
                as u64;
        let mut current_offset = core::mem::size_of::<crate::boot::boot_info::Madt>() as u64;

        while current_offset < total_table_length {
            let record_virt_addr = x86_64::VirtAddr::new(madt_virt.as_u64() + current_offset);
            let record_header = unsafe {
                &*(record_virt_addr.as_ptr::<crate::boot::boot_info::MadtRecordHeader>())
            };

            if record_header.record_length < 2 {
                crate::serial_println!("Reached zero-padded region of MADT. Breaking loop.");
                break;
            }

            if record_header.entry_type == 1 {
                let io_apic_record = unsafe {
                    &*(record_virt_addr.as_ptr::<crate::boot::boot_info::IoApicRecord>())
                };
                let io_apic_addr = unsafe {
                    core::ptr::read_unaligned(core::ptr::addr_of!(io_apic_record.io_apic_address))
                };
                crate::serial_println!("Found IO APIC Physical Address: {:#x}", io_apic_addr);

                let io_apic_phys = x86_64::PhysAddr::new(io_apic_addr as u64);
                let io_apic_virt =
                    x86_64::VirtAddr::new(io_apic_addr as u64 + crate::mm::paging::PHYS_OFFSET);

                crate::mm::paging::map_to(
                    x86_64::structures::paging::Page::<Size4KiB>::containing_address(io_apic_virt),
                    x86_64::structures::paging::PhysFrame::<Size4KiB>::containing_address(
                        io_apic_phys,
                    ),
                    mmio_flags,
                    active_table,
                )
                .expect("Failed to map I/O APIC");

                // IO APIC abstraction
                let io_apic = unsafe { crate::interrupts::io_apic::IoApic::new(io_apic_virt) };
                *crate::interrupts::io_apic::IO_APIC.lock() = Some(io_apic);

                break;
            }
            current_offset += record_header.record_length as u64;
        }
    } else {
        panic!("FATAL: No MADT (APIC) found in ACPI tables!");
    }

    let p4_table = crate::mm::paging::active_level_4_table();

    // Remove 0x0 identity mapping

    // Set p4 index 0 to 0
    p4_table[0].set_unused();

    // Flush the TLB fully so the CPU completely forgets about the old mapping
    x86_64::instructions::tlb::flush_all();

    serial_println!("Successfully severed the identity mapping");

    // Setup guard page
    // Setup guard page using the VIRTUAL address!
    let virtual_st_bottom = &raw const stack_bottom as u64;
    let guard_page_addr = x86_64::VirtAddr::new(virtual_st_bottom - 4096);
    let guard_page: x86_64::structures::paging::Page =
        x86_64::structures::paging::Page::containing_address(guard_page_addr);

    crate::serial_println!("Mapping guard page");
    if let Some(_page) = crate::mm::paging::unmap(guard_page, p4_table) {
        serial_println!("Successfully unmapped the guard page");
    } else {
        serial_println!("Failed to unmap the guard page");
    }

    use x86_64::VirtAddr;
    // Take absure virtual address for testing
    let virt_addr = VirtAddr::new(0x1000_0000_0000);
    let page = x86_64::structures::paging::Page::containing_address(virt_addr);

    //  Ask our bump allocator for a fresh physical frame to back it
    let physical_frame_addr = crate::mm::memory::allocate_frame().unwrap();
    let frame = x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(
        physical_frame_addr as u64,
    ));

    //  Map them together!
    use x86_64::structures::paging::PageTableFlags;
    crate::mm::paging::map_to(page, frame, PageTableFlags::WRITABLE, p4_table)
        .expect("Failed to map page");

    // Test the mapping by writing to the VIRTUAL address
    crate::serial_println!("Mapping successful! Writing to virtual address...");
    let page_ptr = virt_addr.as_mut_ptr::<u64>();
    unsafe {
        // Write a recognizable hex value
        *page_ptr = 0xABCDEF;
    }

    crate::serial_println!("Successfully read back: {:#X}", unsafe { *page_ptr });

    // Initalize the heap
    crate::mm::memory::init_heap(p4_table);

    // Give pages to the heap
    unsafe {
        crate::mm::allocator::ALLOCATOR
            .lock()
            .init(crate::mm::memory::HEAP_START, crate::mm::memory::HEAP_SIZE);
    }

    crate::serial_println!("Heap initialized");

    crate::arch::x86_64::cpu::init_cpu_local();
    crate::process::syscall::init();

    // crate::drivers::vga_buffer::WRITER.lock().clear();

    let mut test_string = alloc::string::String::new();
    test_string.push_str("Hello now");
    println!("Test string: {:?}", test_string);

    use x86_64::registers::control::Cr3;
    let (level_4_page_table_frame, _flags) = Cr3::read();
    let kernel_cr3_phys = level_4_page_table_frame.start_address().as_u64();

    let process_0 = crate::process::process::Process {
        pid: 0,
        page_table: kernel_cr3_phys,
    };

    let mut sched = crate::process::process::SCHEDULER.lock();

    sched.processes.push(Some(process_0));

    let idle_task = crate::process::process::Thread::new(
        &mut sched,
        crate::process::process::idle_task as *const () as u64,
        1024,
    );

    sched.set_idle_task(idle_task);
    drop(sched);

    let user_code: [u8; 12] = [
        0xB8, 0x3C, 0x00, 0x00, 0x00, 0xBF, 0x2A, 0x00, 0x00, 0x00, 0x0F, 0x05,
    ];

    crate::serial_println!("Spawning isolated Ring 3 user process...");
    crate::process::process::spawn_user_process(&user_code, 1024);

    // crate::process::process::spawn(crate::process::process::task_a, 1024);
    // crate::process::process::spawn(crate::process::process::task_b, 1024);

    // Disable the legacy PIC
    // Since its hardware timer is mapped to IRQ 0, which is mapped to Vector 8
    // In modern x86_64 vector 8 is used for double faults so it will esentially
    // instantly crash if not disabled
    // After disabling the legacy PIC, it will be mapped to Vector 32
    unsafe { crate::interrupts::apic::disable_legacy_pic() };

    crate::interrupts::apic::LOCAL_APIC
        .lock()
        .as_ref()
        .unwrap()
        .calibrate_and_start_timer();

    // Set the CPU's Interrupt Flag (sti) so it actually listens to the APIC
    x86_64::instructions::interrupts::enable();

    // Unmask the keyboard!
    unsafe {
        crate::interrupts::io_apic::IO_APIC
            .lock()
            .as_ref()
            .unwrap()
            .init_keyboard();
    }

    crate::serial_println!("Interrupts enabled. Waiting for keyboard input...");
    loop {
        x86_64::instructions::hlt();
    }
    // loop {
    //     // Pop events off the queue and print them!
    //     if let Some(key_event) = crate::drivers::keyboard::KEYBOARD_MAILBOX.receive() {
    //         match key_event {
    //             pc_keyboard::DecodedKey::Unicode(character) => crate::print!("{}", character),
    //             pc_keyboard::DecodedKey::RawKey(key) => crate::print!("{:?}", key),
    //         }
    //     } else {
    //         // If the queue is empty, put the CPU to sleep to save power
    //         x86_64::instructions::hlt();
    //     }
    // }
}
