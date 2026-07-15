// src/interrupt.rs

use crate::println;
use spin::Once;
use x86_64::structures::idt::InterruptDescriptorTable;

/// Lazily-initialised, globally-shared Interrupt Descriptor Table.
///
/// Wrapped in [`spin::Once`] so that `load_idt` can safely build and load
/// the IDT exactly once, even if called from multiple code paths.
// static IDT_INIT: Once<InterruptDescriptorTable> = Once::new();
static IDT_INIT: Once<InterruptDescriptorTable> = Once::new();

/// Builds and loads the kernel's Interrupt Descriptor Table (IDT).
///
/// Uses [`spin::Once`] to guarantee the IDT is constructed and `lidt`-loaded
/// exactly once, even if `load_idt` is called multiple times.
///
/// # Registered handlers
///
/// | Vector | Name | Handler |
/// |--------|------|---------|
/// | 3 | Breakpoint (`#BP`) | [`breakpoint_handler`] |
/// | 6 | Invalid Opcode (`#UD`) | [`invaild_opcode_handler`] |
/// | 8 | Double Fault (`#DF`) | [`double_fault_handler`] on IST 0 |
/// | 12 | Stack-Segment Fault (`#SS`) | [`stack_segment_fault_handler`] |
/// | 13 | General Protection Fault (`#GP`) | [`general_protection_failure_handler`] |
/// | 14 | Page Fault (`#PF`) | [`page_fault_handler`] |
/// | 2 | NMI | [`non_maskable_interrupt_handler`] |
/// | 32 | APIC Timer | `timer_isr` (raw assembly) |
/// | 33 | Keyboard | [`keyboard_handler`] |
pub fn load_idt() {
    let idt = IDT_INIT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        // Interrupt handler for managing the timer interrupt
        unsafe {
            idt[32].set_handler_addr(x86_64::VirtAddr::new(timer_isr as *const () as u64));
        }
        idt[33].set_handler_fn(keyboard_handler);
        // General Protection Fault (#GP - Vector 13): Catches memory protection
        // and privilege violations, such as accessing non-canonical addresses or
        // a user-space program trying to touch kernel memory.
        idt.general_protection_fault
            .set_handler_fn(general_protection_failure_handler);
        // Stack Segment Fault (#SS - Vector 12): Catches errors strictly related to the
        // stack, such as the stack pointer (rsp) becoming corrupted, misaligned, or
        // overflowing its mapped memory.
        idt.stack_segment_fault
            .set_handler_fn(stack_segment_fault_handler);
        // Invalid Opcode (#UD - Vector 6): Catches illegal or unknown CPU instructions,
        // which almost always happens when a bad pointer causes the CPU to jump into
        // random data and try to execute it as code.
        idt.invalid_opcode.set_handler_fn(invaild_opcode_handler);
        // Non-Maskable Interrupt (#NMI - Vector 2): Catches catastrophic, unrecoverable
        // physical hardware errors (like RAM parity failures) and bypasses the CPU's
        // interrupt flag (sti/cli) so it cannot be ignored.
        idt.non_maskable_interrupt
            .set_handler_fn(non_maskable_interrupt_handler);
        // Double Fault
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt
    });
    idt.load();
}

use x86_64::structures::idt::PageFaultErrorCode;

/// Page-fault handler — implements demand paging for the kernel heap.
///
/// # Behaviour
///
/// 1. Reads the faulting virtual address from `CR2`.
/// 2. If the address is **outside** the heap window `[HEAP_START, HEAP_START+HEAP_SIZE)`,
///    the fault is a genuine kernel bug: dumps the stack frame and error code on
///    serial + VGA and halts the CPU.
/// 3. If the address is **inside** the heap, the fault is a valid demand-paging
///    miss (the `HeapAllocator` bumped the virtual pointer without mapping the
///    page).  Allocates a zeroed physical frame, maps the faulting page with
///    `PRESENT | WRITABLE` flags, and returns to retry the faulting instruction.
extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    // Faulting address
    let fault_addr = x86_64::registers::control::Cr2::read();
    let fault_u64 = fault_addr.as_u64();

    let heap_start = crate::mm::memory::HEAP_START as u64;
    let heap_size = crate::mm::memory::HEAP_SIZE as u64;
    let is_kernel_heap = fault_u64 >= heap_start && fault_u64 < heap_start + heap_size;

    let mut is_user_heap = false;
    let mut is_user_stack = false;

    if !is_kernel_heap {
        use core::sync::atomic::Ordering;
        let heap_start = crate::process::process::CURRENT_BOUNDS
            .heap_start
            .load(Ordering::Relaxed);
        let program_break = crate::process::process::CURRENT_BOUNDS
            .program_break
            .load(Ordering::Relaxed);
        let stack_start = crate::process::process::CURRENT_BOUNDS
            .stack_start
            .load(Ordering::Relaxed);
        let stack_end = crate::process::process::CURRENT_BOUNDS
            .stack_end
            .load(Ordering::Relaxed);

        if fault_u64 >= heap_start && fault_u64 < program_break {
            is_user_heap = true;
        } else if fault_u64 >= stack_start && fault_u64 < stack_end {
            is_user_stack = true;
        }
    }

    if !is_kernel_heap && !is_user_heap && !is_user_stack {
        // crate::serial_print!(
        //     "EXCEPTION: PAGE FAULT\n{:#?}\n Error code: {:#?}",
        //     stack_frame,
        //     error_code
        // );
        // crate::serial_println!(
        //     "EXPECTION: PAGE FAULT: OCCURED AT: {:#x}",
        //     fault_addr.as_u64()
        // );
        // println!(
        //     "EXCEPTION: PAGE FAULT\n{:#?}\n Error code: {:#?}",
        //     stack_frame, error_code
        // );
        crate::serial_println!(
            "EXCEPTION: PAGE FAULT OCCURRED AT: {:#x}",
            fault_addr.as_u64()
        );
        crate::serial_println!(
            "Instruction Pointer: {:#x}",
            stack_frame.instruction_pointer.as_u64()
        );
        crate::serial_println!("Error code: {:?}", error_code);
        loop {
            x86_64::instructions::hlt();
        }
    }

    crate::serial_println!("Demand paging: Allocating fresh page for heap");

    let page: x86_64::structures::paging::Page =
        x86_64::structures::paging::Page::containing_address(fault_addr);

    // Get 4kb zeroed frame from bitmap allocator
    let frame_addr =
        crate::mm::memory::allocate_zeroed_frame().expect("FATAL ERROR: Out of memory");

    let physical_addr: x86_64::structures::paging::PhysFrame =
        x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(
            frame_addr as u64,
        ));

    // Get active page table and map it
    let active_table = crate::mm::paging::active_level_4_table();
    let mut flags = x86_64::structures::paging::PageTableFlags::PRESENT
        | x86_64::structures::paging::PageTableFlags::WRITABLE
        | x86_64::structures::paging::PageTableFlags::NO_EXECUTE;

    if is_user_heap || is_user_stack {
        flags |= x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE;
    }

    crate::mm::paging::map_to(page, physical_addr, flags, active_table)
        .expect("FATAL: demand paging failed to map page");

    return;
}

use x86_64::structures::idt::InterruptStackFrame;

/// Breakpoint (`#BP`, vector 3) handler — prints the interrupt stack frame and returns.
///
/// Used for software debugging (`int3` instructions).  The CPU automatically
/// adjusts `RIP` to point past the `int3` opcode, so execution continues
/// normally after this handler returns.
// This is the handler for the interrupt.
extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

/// Double-fault (`#DF`, vector 8) handler — panics with the stack frame.
///
/// A double fault occurs when an exception fires while the CPU is already
/// processing another exception and has no handler for it (or the handler
/// itself faults).  This is almost always a sign of a corrupted kernel stack
/// or a missing IDT entry.  The handler runs on the dedicated IST stack
/// (slot [`crate::arch::x86_64::gdt::DOUBLE_FAULT_IST_INDEX`]) to survive a
/// stack-overflow scenario.
extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    let mut serial_port = unsafe { uart_16550::SerialPort::new(0x3F8) };
    use core::fmt::Write;
    let _ = writeln!(
        serial_port,
        "FATAL: DOUBLE FAULT AT {:#x}",
        stack_frame.instruction_pointer
    );
    panic!(
        "FATAL: DOUBLE FAULT AT {:#x}",
        stack_frame.instruction_pointer
    );
}

unsafe extern "C" {
    /// External C declaration for the raw assembly timer ISR stub defined in
    /// `process::process` via `core::arch::global_asm!`.
    ///
    /// The stub saves the full `ThreadContext` on the stack, calls
    /// `process::rust_timer_handler`, switches `RSP` to the returned context
    /// pointer, restores registers, and executes `iretq`.
    fn timer_isr();
}

/// Keyboard ISR (vector 33) — reads the PS/2 scancode and posts it to the mailbox.
///
/// # Flow
/// 1. Reads one scancode byte from I/O port `0x60`.
/// 2. Passes it to the `pc-keyboard` state machine ([`crate::drivers::keyboard::KEYBOARD`]).
/// 3. If the state machine produces a decoded key event, sends it to
///    [`crate::drivers::keyboard::KEYBOARD_MAILBOX`] for consumption by
///    user-space tasks or the kernel event loop.
/// 4. Sends EOI to the Local APIC so future interrupts are not blocked.
extern "x86-interrupt" fn keyboard_handler(_stack_frame: InterruptStackFrame) {
    use x86_64::instructions::port::Port;

    // The keyboard data port is 0x60
    let mut port = Port::<u8>::new(0x60);

    let scancode: u8 = unsafe { port.read() };

    crate::serial_println!("Key pressed! Scancode: {}", scancode);
    let mut keyboard = crate::drivers::keyboard::KEYBOARD.lock();
    if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
        // If it formed a complete key press/release, process it
        if let Some(key) = keyboard.process_keyevent(key_event) {
            crate::drivers::keyboard::KEYBOARD_MAILBOX.send(key);
        }
    }
    // crate::process::process::SCHEDULER.lock().wake_task(1);
    // CRITICAL: Acknowledge the interrupt to the Local APIC!
    crate::interrupts::apic::LOCAL_APIC
        .lock()
        .as_ref()
        .unwrap()
        .end_of_interrupt();
}

/// General-protection fault (`#GP`, vector 13) handler — panics.
///
/// Triggered by privilege violations (e.g., a user-space program accessing a
/// kernel address, a non-canonical address reference, or a bad segment
/// selector).  Error code encodes which segment was at fault, if any.
extern "x86-interrupt" fn general_protection_failure_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "EXCEPTION: GENERAL PROTECTION FAILURE\n{:#?}\n error code: {:#?}",
        stack_frame, error_code
    );
}

/// Stack-segment fault (`#SS`, vector 12) handler — panics.
///
/// Raised when `RSP` becomes misaligned, wraps around, or addresses unmapped
/// stack memory.  Almost always indicates stack corruption or exhaustion.
extern "x86-interrupt" fn stack_segment_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    panic!(
        "EXCEPTION: STACK SEGMENT FAULT\n{:#?}\n error code: {:#?}",
        stack_frame, error_code
    );
}

/// Invalid-opcode fault (`#UD`, vector 6) handler — panics.
///
/// The CPU raises `#UD` when it encounters an instruction it does not
/// recognise.  Common cause in kernels: a bad function pointer causes a jump
/// into data memory and the CPU tries to decode garbage as opcodes.
extern "x86-interrupt" fn invaild_opcode_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
}

/// Non-maskable interrupt (`#NMI`, vector 2) handler — panics.
///
/// NMIs bypass the CPU's interrupt flag (`IF`) and signal catastrophic,
/// unrecoverable hardware conditions such as uncorrectable ECC RAM errors.
/// They cannot be masked with `cli` and take highest priority.
extern "x86-interrupt" fn non_maskable_interrupt_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: NON MASKABLE INTERRUPT\n{:#?}", stack_frame);
}
