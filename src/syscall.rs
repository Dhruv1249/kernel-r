// src/syscall.rs

unsafe extern "C" {
    fn syscall_entry_stub();
}

pub fn init() {
    unsafe {
        x86_64::registers::model_specific::Efer::write(
            x86_64::registers::model_specific::Efer::read()
                | x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS,
        );
    }

    let kernel_code = crate::gdt::kernel_code_selector();
    let kernel_data = crate::gdt::kernel_data_selector();
    let user_data = crate::gdt::user_data_selector();
    let user_code = crate::gdt::user_code_selector();

    // The hardware math subtracts 8 from the user data selector to find the 32-bit base.

    x86_64::registers::model_specific::Star::write(user_code, user_data, kernel_code, kernel_data).expect("FATAL: Invalid segment selectors passed to STAR MSR");

    x86_64::registers::model_specific::LStar::write(x86_64::VirtAddr::new(
        syscall_entry_stub as usize as u64,
    ));

    // When syscall occurs, we want to disable hardware interrupts automatically.
    x86_64::registers::model_specific::SFMask::write(
        x86_64::registers::rflags::RFlags::INTERRUPT_FLAG,
    );
}

core::arch::global_asm!(
    ".global syscall_entry_stub",
    "syscall_entry_stub:",
    // Swap GS to access our PerCpu struct
    "swapgs",
    
    // Save the dangerous user stack to our scratchpad (Offset 16)
    "mov gs:[16], rsp",
    
    // Load the safe kernel stack (Offset 8)
    "mov rsp, gs:[8]",

    // Construct the ThreadContext. 
    
    // Push SS (User Data Segment: Index 4 * 8 | Ring 3 = 0x23)
    "push 0x23",
    // Push RSP (The user stack we saved in the scratchpad)
    "push qword ptr gs:[16]",
    // Push RFLAGS (syscall hardware saves this in R11)
    "push r11",
    // Push CS (User Code Segment: Index 5 * 8 | Ring 3 = 0x2B)
    "push 0x2B",
    // Push RIP (syscall hardware saves this in RCX)
    "push rcx",
    // Push a dummy error code to match the struct size
    "push 0",

    // Push all General Purpose Registers
    "push rax",
    "push rcx", 
    "push rdx",
    "push rbx",
    "push rbp",
    "push rsi",
    "push rdi",
    "push r8",
    "push r9",
    "push r10", // Note: Syscalls usually pass the 4th argument in r10, not rcx!
    "push r11", 
    "push r12",
    "push r13",
    "push r14",
    "push r15",

    // Call the Rust handler
    "mov rdi, rsp", // Pass the pointer to the ThreadContext as the first argument
    "call rust_syscall_handler",

    // Restore the state
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
    "pop rax", // RAX contains the return value of our syscall!

    // Pop the dummy error code
    "add rsp, 8",
    
    // SYSRET requires RIP in RCX, and RFLAGS in R11
    "pop rcx", // Pop RIP into RCX
    "add rsp, 8", // Discard CS
    "pop r11", // Pop RFLAGS into R11
    
    // Restore the user's stack
    "pop rsp", 
    
    // We are now back on the user's stack! We must swap GS back and return immediately.
    "swapgs",
    "sysretq",
);

#[unsafe(no_mangle)]
pub extern "C" fn rust_syscall_handler(context: &mut crate::process::ThreadContext) {
    let syscall_no = context.rax;

    match syscall_no {
        // SYS_WRITE (1)
        // arg1 (rdi) = file descriptor (1 is stdout)
        // arg2 (rsi) = virtual address of string buffer in user space
        // arg3 (rdx) = length of string
        1 => {
            let fd = context.rdi;
            let buf_ptr = context.rsi as *const u8;
            let len = context.rdx as usize;

            if fd == 1 || fd == 2 {
                // Read the string safely from user memory
                // (In a hardened OS, you'd validate that buf_ptr is a valid Ring 3 address first!)
                let slice = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
                if let Ok(s) = core::str::from_utf8(slice) {
                    crate::print!("{}", s);
                    crate::serial_print!("{}", s);
                }
                context.rax = len as u64; // Return number of bytes written
            } else {
                context.rax = (!(9u64)).wrapping_add(1); // -9 (EBADF - Bad File Descriptor)
            }
        }

        // SYS_EXIT (60)
        // arg1 (rdi) = exit code
        60 => {
            let exit_code = context.rdi;
            crate::serial_println!("User thread exited with code: {}", exit_code);
            
            crate::process::exit_thread();
        }

        // Unknown Syscall
        _ => {
            crate::serial_println!("Unknown Syscall: {}", syscall_no);
            context.rax = (!(38u64)).wrapping_add(1); // -38 (ENOSYS - Function not implemented)
        }
    } 
}
