// src/interrupt.rs

use crate::println;
use spin::Once;
use x86_64::structures::idt::InterruptDescriptorTable;

// static IDT_INIT: Once<InterruptDescriptorTable> = Once::new();
static IDT_INIT: Once<InterruptDescriptorTable> = Once::new();

pub fn load_idt() {
    let idt = IDT_INIT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt[32].set_handler_fn(tick_handler);
        idt[33].set_handler_fn(keyboard_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt
    });
    idt.load();
}

use x86_64::structures::idt::PageFaultErrorCode;
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // Faulting address
    let fault_addr = x86_64::registers::control::Cr2::read();

    let heap_start = crate::memory::HEAP_START as u64;
    let heap_size = crate::memory::HEAP_SIZE as u64;
    if fault_addr.as_u64() < heap_start || fault_addr.as_u64() >= heap_start + heap_size {
        crate::serial_print!("EXCEPTION: PAGE FAULT\n{:#?}\n Error code: {:#?}", stack_frame, error_code);
        crate::serial_println!("EXPECTION: PAGE FAULT: OCCURED AT: {:#x}", fault_addr.as_u64());
        println!("EXCEPTION: PAGE FAULT\n{:#?}\n Error code: {:#?}", stack_frame, error_code);
        loop {
            x86_64::instructions::hlt();
        }
    }

    crate::serial_println!("Demand paging: Allocating fresh page for heap");

    let page: x86_64::structures::paging::Page<> = x86_64::structures::paging::Page::containing_address(
        fault_addr
    );
    
    // Get 4kb zeroed frame from bitmap allocator
    let frame_addr = crate::memory::allocate_zeroed_frame().expect("FATAL ERROR: Out of memory");

    let physical_addr: x86_64::structures::paging::PhysFrame<> = x86_64::structures::paging::PhysFrame::containing_address(
        x86_64::PhysAddr::new(frame_addr as u64)
    );

    // Get active page table and map it
    let active_table = crate::paging::active_level_4_table();
    let flags = x86_64::structures::paging::PageTableFlags::PRESENT| x86_64::structures::paging::PageTableFlags::WRITABLE;

    crate::paging::map_to(page, physical_addr, flags, active_table)
        .expect("FATAL: demand paging failed to map page");

    return;
}

use x86_64::structures::idt::InterruptStackFrame;

// This is the handler for the interrupt.
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}


extern "x86-interrupt" fn tick_handler(stack_frame: InterruptStackFrame) {
    // crate::print!(".");
    crate::apic::LOCAL_APIC.lock().as_ref().unwrap().end_of_interrupt();
}

extern "x86-interrupt" fn keyboard_handler(stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    // The keyboard data port is 0x60
    let mut port = Port::<u8>::new(0x60);
    
    let scancode: u8 = unsafe { port.read() };
    
    crate::serial_println!("Key pressed! Scancode: {}", scancode);
    let mut keyboard = crate::keyboard::KEYBOARD.lock();
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        // 2. If it formed a complete key press/release, process it
        if let Some(key) = keyboard.process_keyevent(key_event) {
            match key {
                pc_keyboard::DecodedKey::Unicode(character) => crate::print!("{}", character),
                pc_keyboard::DecodedKey::RawKey(key) => crate::print!("{:?}", key),
            }
        }
    }    // CRITICAL: Acknowledge the interrupt to the Local APIC!
    crate::apic::LOCAL_APIC.lock().as_ref().unwrap().end_of_interrupt();
}
