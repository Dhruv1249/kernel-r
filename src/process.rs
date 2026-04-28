// src/process.rs

use alloc::vec;

#[derive(Debug, Default, Clone, Copy)]
#[repr(C)] // CRITICAL: Must be C-representation so our Assembly can read it reliably
pub struct TaskContext {
    // We need to preserve these registers across task switches
    pub rbx: u64,
    pub rbp: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
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
        let initial_rsp = stack_end - 16;

        unsafe {
            core::ptr::write((stack_end - 8) as *mut u64, 0);
            core::ptr::write(initial_rsp as *mut u64, entry_point);
        }
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


unsafe extern "C" {
    pub fn switch_task(
        old_context: *mut TaskContext, // rdi
        old_sp: *mut u64,              // rsi
        new_context: *const TaskContext, // rdx
        new_sp: u64,                   // rcx
    );
}

core::arch::global_asm!(
    ".global switch_task",
    "switch_task:",

    // Save the old context
    "mov [rdi + 0x0], rbx",
    "mov [rdi + 0x8], rbp",
    "mov [rdi + 0x10], r12",
    "mov [rdi + 0x18], r13",
    "mov [rdi + 0x20], r14",
    "mov [rdi + 0x28], r15",

    // Save the old stack pointer
    "mov [rsi], rsp",

    // Load the new stack pointer
    "mov rsp, rcx",

    // Load the new context
    "mov rbx, [rdx + 0x0]",
    "mov rbp, [rdx + 0x8]",
    "mov r12, [rdx + 0x10]",
    "mov r13, [rdx + 0x18]",
    "mov r14, [rdx + 0x20]",
    "mov r15, [rdx + 0x28]",

    // Return
    "ret",
);




