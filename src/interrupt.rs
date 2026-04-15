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
    use x86_64::registers::control::Cr2;
    crate::serial_println!("EXCEPTION: PAGE FAULT");
    crate::serial_println!("Accessed Address: {:?}", Cr2::read());
    crate::serial_println!("Error Code: {:?}", error_code);
    crate::serial_println!("{:#?}", stack_frame);
    loop {}
}

use x86_64::structures::idt::InterruptStackFrame;

// This is the handler for the interrupt.
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    // crate::exit_qemu(crate::qemu::QemuExitCode::Failure);
    crate::serial_println!("EXCEPTION: DOUBLE FAULT");
    crate::serial_println!("Error Code: {:#x}", error_code);
    crate::serial_println!("{:#?}", stack_frame);
    crate::println!("EXCEPTION: DOUBLE FAULT");
    // crate::exit_qemu(crate::qemu::QemuExitCode::Success);
    loop {}
}
