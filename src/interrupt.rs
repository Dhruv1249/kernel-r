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
        idt
    });
    idt.load();
}


use x86_64::structures::idt::InterruptStackFrame;

// This is the handler for the interrupt.
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
   println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}
