// src/syscall.rs

unsafe extern "C" {
    fn syscall_entry_stub();
}

/// Configures the SYSCALL/SYSRET MSRs and enables the SYSCALL instruction.
///
/// # MSR configuration
///
/// | MSR | Purpose | Value written |
/// |-----|---------|---------------|
/// | `EFER` | Enable SYSCALL/SYSRET | Set `SYSTEM_CALL_EXTENSIONS` bit |
/// | `STAR` | Selector routing | Kernel CS = Ring0 code sel, User CS = Ring3 code sel |
/// | `LSTAR` | 64-bit SYSCALL entry point | `syscall_entry` (assembly stub) |
/// | `SFMASK` | RFLAGS bits to clear on SYSCALL | `0x200` (interrupt flag) |
///
/// After `init`, any `syscall` instruction executed in Ring 3 will:
/// 1. Disable interrupts (RFLAGS.IF cleared via SFMASK).
/// 2. Save user `RIP` in `RCX` and user `RFLAGS` in `R11`.
/// 3. Jump to `syscall_entry` in kernel mode.
/// 4. Switch stacks using the per-CPU `GS`-relative `kernel_rsp` field.
pub fn init() {
    unsafe {
        x86_64::registers::model_specific::Efer::write(
            x86_64::registers::model_specific::Efer::read()
                | x86_64::registers::model_specific::EferFlags::SYSTEM_CALL_EXTENSIONS,
        );
    }

    let kernel_code = crate::arch::x86_64::gdt::kernel_code_selector();
    let kernel_data = crate::arch::x86_64::gdt::kernel_data_selector();
    let user_data = crate::arch::x86_64::gdt::user_data_selector();
    let user_code = crate::arch::x86_64::gdt::user_code_selector();

    // The hardware math subtracts 8 from the user data selector to find the 32-bit base.

    x86_64::registers::model_specific::Star::write(user_code, user_data, kernel_code, kernel_data)
        .expect("FATAL: Invalid segment selectors passed to STAR MSR");

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
    "pop rcx",    // Pop RIP into RCX
    "add rsp, 8", // Discard CS
    "pop r11",    // Pop RFLAGS into R11
    // Restore the user's stack
    "pop rsp",
    // We are now back on the user's stack! We must swap GS back and return immediately.
    "swapgs",
    "sysretq",
);

/// The Rust-side syscall dispatch handler, called by the `syscall_entry` stub.
///
/// The `syscall_entry` assembly stub (defined below via `global_asm!`) saves
/// the user-mode register state into a [`crate::process::process::ThreadContext`]
/// on the kernel stack, then calls this function with a mutable reference to
/// that context.  On return, `syscall_entry` restores the (possibly modified)
/// context and executes `sysretq` back to user space.
///
/// # Dispatch table
///
/// | `context.rax` | Syscall | Action |
/// |---------------|---------|--------|
/// | `42` | `print` | Print `rdi`-length UTF-8 bytes at `rsi` to VGA + serial |
/// | `60` | `exit` | Mark thread zombie and context-switch away |
/// | _other_ | unknown | Log on serial and return |
#[unsafe(no_mangle)]
pub extern "C" fn rust_syscall_handler(context: &mut crate::process::process::ThreadContext) {
    let syscall_no = context.rax;

    let arg1 = context.rdi;
    let arg2 = context.rsi;
    let arg3 = context.rdx;
    let arg4 = context.r10;
    let arg5 = context.r8;
    let arg6 = context.r9;

    match syscall_no {
        // SYS_READ (0)
        0 => {
            let fd = arg1 as usize;
            let buf_ptr = arg2 as *mut u8;
            let len = arg3 as usize;

            if fd >= crate::process::process::MAX_FDS {
                context.rax = (!(9u64)).wrapping_add(1); // EBADF
            } else {
                let file_arc = {
                    let mut sched = crate::process::process::SCHEDULER.lock();
                    let current_task_id = sched.current_task.expect("FATAL: No current task!");
                    let pid = sched.tasks.get_mut(current_task_id).unwrap().pid;
                    let process = sched.processes.get(pid as usize).unwrap().as_ref().unwrap();
                    process.fd_table[fd as usize].clone()
                };

                if let Some(file_arc) = file_arc {
                    let mut file: spin::MutexGuard<crate::fs::vfs::OpenFile> = file_arc.lock();

                    if file.readable {
                        let slice = unsafe { core::slice::from_raw_parts_mut(buf_ptr, len) };
                        if let Some(bytes_read) = file.vnode.read(file.offset, slice) {
                            file.offset += bytes_read;
                            context.rax = bytes_read as u64;
                        } else {
                            context.rax = (!(9u64)).wrapping_add(1); // EBADF
                        }
                    } else {
                        context.rax = (!(9u64)).wrapping_add(1); // EBADF
                    }
                } else {
                    context.rax = (!(9u64)).wrapping_add(1); // EBADF
                }
            }
        }

        // SYS_WRITE (1)
        // arg1 (rdi) = file descriptor (1 is stdout)
        // arg2 (rsi) = virtual address of string buffer in user space
        // arg3 (rdx) = length of string
        1 => {
            let fd = arg1 as usize;
            let buf_ptr = arg2 as *const u8;
            let len = arg3 as usize;

            if fd >= crate::process::process::MAX_FDS {
                context.rax = (!(9u64)).wrapping_add(1); // EBADF
            } else {
                let file_arc = {
                    let mut sched = crate::process::process::SCHEDULER.lock();
                    let current_task_id = sched.current_task.expect("FATAL: No current task!");
                    let pid = sched.tasks.get_mut(current_task_id).unwrap().pid;
                    let process = sched.processes.get(pid as usize).unwrap().as_ref().unwrap();
                    process.fd_table[fd as usize].clone()
                };
                if let Some(file_arc) = file_arc {
                    let mut file: spin::MutexGuard<crate::fs::vfs::OpenFile> = file_arc.lock();

                    if file.writable {
                        let slice = unsafe { core::slice::from_raw_parts(buf_ptr, len) };
                        if let Some(bytes_written) = file.vnode.write(file.offset, slice) {
                            file.offset += bytes_written;
                            context.rax = bytes_written as u64;
                        } else {
                            context.rax = (!(9u64)).wrapping_add(1); // EBADF
                        }
                    } else {
                        context.rax = (!(9u64)).wrapping_add(1); // EBADF
                    }
                } else {
                    context.rax = (!(9u64)).wrapping_add(1); // EBADF
                }
            }
        }

        // SYS_OPEN (2)
        2 => {
            let filename_ptr = arg1 as *const u8;

            let mut len = 0;
            unsafe {
                for i in 0..256 {
                    if *(filename_ptr.add(i)) == 0 {
                        len = i;
                        break;
                    }
                }
            }

            if len == 0 {
                context.rax = (!(2u64)).wrapping_add(1); // ENOENT (Bad filename string)
            } else {
                let slice = unsafe { core::slice::from_raw_parts(filename_ptr, len) };
                if let Ok(filename) = core::str::from_utf8(slice) {
                    let root_lock = crate::fs::vfs::ROOT_FS.lock();

                    if let Some(root_dir) = root_lock.as_ref() {
                        if let Some(vnode) = root_dir.lookup(filename) {
                            let mut sched = crate::process::process::SCHEDULER.lock();
                            let current_task_id =
                                sched.current_task.expect("FATAL: No current task!");
                            let pid = sched.tasks.get_mut(current_task_id).unwrap().pid;
                            let process = sched
                                .processes
                                .get_mut(pid as usize)
                                .unwrap()
                                .as_mut()
                                .unwrap();

                            let mut assigned_fd = None;
                            for i in 3..crate::process::process::MAX_FDS {
                                if process.fd_table[i].is_none() {
                                    assigned_fd = Some(i);
                                    break;
                                }
                            }

                            if let Some(fd) = assigned_fd {
                                process.fd_table[fd] = Some(alloc::sync::Arc::new(
                                    spin::Mutex::new(crate::fs::vfs::OpenFile {
                                        vnode,
                                        offset: 0,
                                        readable: true,
                                        writable: false,
                                    }),
                                ));
                                context.rax = fd as u64; // Success! Return the FD.
                            } else {
                                context.rax = (!(24u64)).wrapping_add(1); // EMFILE
                            }
                        } else {
                            context.rax = (!(2u64)).wrapping_add(1); // ENOENT (Not found)
                        }
                    } else {
                        context.rax = (!(2u64)).wrapping_add(1); // ENOENT (No Root FS)
                    }
                } else {
                    context.rax = (!(2u64)).wrapping_add(1); // ENOENT (Bad UTF-8)
                }
            }
        }

        // SYS_EXIT (60)
        // arg1 (rdi) = exit code
        60 => {
            let exit_code = arg1;
            crate::serial_println!("User thread exited with code: {}", exit_code);

            crate::process::process::exit_thread();
        }

        12 => {
            let requested_break = arg1;

            let mut sched = crate::process::process::SCHEDULER.lock();

            let curent_task_id = sched.current_task.expect("FATAL: No current task!");

            let pid = sched.tasks.get_mut(curent_task_id).unwrap().pid;

            let process: &mut crate::process::process::Process = sched
                .processes
                .get_mut(pid as usize)
                .unwrap()
                .as_mut()
                .unwrap();

            if requested_break == 0 || requested_break < process.heap_start {
                context.rax = process.program_break;
            } else {
                process.program_break = requested_break;
                context.rax = requested_break;
            }

        }

        // Unknown Syscall
        _ => {
            crate::serial_println!("Unknown Syscall: {}", syscall_no);
            context.rax = (!(38u64)).wrapping_add(1); // -38 (ENOSYS - Function not implemented)
        }
    }
}
