use x86_64::structures::idt::InterruptDescriptorTable;
use crate::println; 
use spin::Once;


static IDT_INIT: Once<InterruptDescriptorTable> = Once::new();


// Wanted to use lazy_static but it was just crashing the kernel for some reason.
pub fn load_id() {
    // This ensures that the IDT is only initialized once.
    let idt = IDT_INIT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt
    });
    idt.load();
}


use x86_64::structures::idt::InterruptStackFrame;

// This is the handler for the interrupt.
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
   println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error_code: u64) -> ! {
    println!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
    // Error code will always be 0 so no need to print it.
    // println!("Error Code: {:#x}", error_code);
    loop {}
}
