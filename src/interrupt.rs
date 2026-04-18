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
    panic!("EXCEPTION: PAGE FAULT\n{:#?}\n Error code: {:#?}", stack_frame, error_code);
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
