// src/process.rs

use alloc::vec;

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)] // CRITICAL: Must be C-representation so our Assembly can read it reliably
pub struct TaskContext {
    // Order in which registers are pushed onto the stack
    // r15 is pushed last while ss is pushed first
    pub r15: u64, // Preserved
    pub r14: u64, // Preserved
    pub r13: u64, // Preserved
    pub r12: u64, // Preserved
    pub r11: u64, // Scratch
    pub r10: u64, // Scratch
    pub r9: u64,  // Scratch
    pub r8: u64,  // Scratch
    pub rdi: u64, // Scratch
    pub rsi: u64, // Scratch
    pub rbp: u64, // Preserved
    pub rbx: u64, // Preserved
    pub rdx: u64, // Scratch
    pub rcx: u64, // Scratch
    pub rax: u64, // Scratch
    pub error_code: u64,
    // Hardware Frames
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

pub enum TaskState {
    Running,
    Ready,
    Sleeping,
}

pub struct Task {
    pub id: u64,
    pub stack_pointer: u64, // The physical location of the RSP register
    pub context: TaskContext,
    pub state: TaskState, // e.g., Running, Ready, Sleeping
    pub page_table: u64,  // The CR3 register (for when we add User Space later)
    pub stack: alloc::vec::Vec<u8>,
}

const TASK_STACK_SIZE: usize = 0x400 * 16; // 16 KB

impl Task {
    pub fn new(entry_point: u64) -> Self {
        // Allocte memory for the stack
        let mut stack =  vec![0; TASK_STACK_SIZE];

        // Get the highest stack address
        let stack_start = stack.as_mut_ptr() as u64;
        let stack_end = stack_start + TASK_STACK_SIZE as u64;

        // Step back 16 bytes to make room to write a 64-bit return address
        let initial_rsp = stack_end - core::mem::size_of::<TaskContext>() as u64;

        let context = TaskContext{
            rip: entry_point,
            cs: 0x8,
            rflags: 0x202,
            rsp: initial_rsp,
            ss: 0x0,
            ..Default::default()
        };
        
        // Write context at the top of the TASK_STACK_SIZE
        unsafe { core::ptr::write(initial_rsp as *mut TaskContext, context); }
        
        Self {
            id: 0,
            stack_pointer: initial_rsp,
            context: TaskContext::default(),
            state: TaskState::Ready,
            page_table: 0,
            stack
        }
    }
}


#[unsafe(no_mangle)]
pub extern "C" fn rust_timer_handler(context: &mut crate::process::TaskContext) -> *mut TaskContext {
    // 1. Tell the APIC we received the interrupt
    crate::apic::LOCAL_APIC.lock().as_ref().unwrap().end_of_interrupt();
    
    crate::serial_println!("Timer tick! Context is at: {:p}", context);
    return context;
}

// Global asm so rust knows how to call this function and doesn't mess with the stack
core::arch::global_asm!(
    // ISR -> Interrupt Service Routine
    ".global timer_isr",
    "timer_isr:",
    // Cpu just pushed ss, rsp, rflags, cs, rip
    // Push error code
    "push 0",
    
    // Push general purpose registers
    "push rax",
    "push rcx",
    "push rdx",
    "push rbx",
    "push rbp",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10",
    "push r11",
    "push r12",
    "push r13",
    "push r14",
    "push r15",

    // Call our rust_timer_handler
    "mov rdi, rsp",
    "call rust_timer_handler",
    // Move our context back to the stack
    "mov rsp, rax",

    // Restore the registers
    "pop r15",
    "pop r14",
    "pop r13",
    "pop r12",
    "pop r11",
    "pop r10",
    "pop r9",
    "pop r8",
    "pop rdi",
    "pop rsi",
    "pop rbp",
    "pop rbx",
    "pop rdx",
    "pop rcx",
    "pop rax",

    // Pop the error code
    "add rsp, 8",

    // Hardware return (tells CPU to pop the ss, rsp, rflags, cs, rip)
    "iretq",

);

