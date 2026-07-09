// src/process/process.rs

/// The full CPU register state saved and restored on every context switch.
///
/// The struct is `#[repr(C)]` so its layout is dictated by the C ABI, making it
/// safe to access from the assembly timer ISR stub (`timer_isr`) which pushes
/// registers in this exact order before calling `rust_timer_handler`.
///
/// The ordering matches the push sequence in `timer_isr`:
/// hardware automatically pushes `ss, rsp, rflags, cs, rip` first, then the
/// ISR stub pushes a dummy error code followed by all general-purpose registers
/// (`rax` through `r15`).  This means `r15` is at the lowest address and `ss`
/// at the highest.
#[derive(Debug, Default, Clone, Copy)]
#[repr(C)] // CRITICAL: Must be C-representation so our Assembly can read it reliably
pub struct ThreadContext {
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

/// The lifecycle state of a kernel thread.
///
/// Transitions:
/// - `Ready` → `Running`: the scheduler picks the thread.
/// - `Running` → `Ready`: the timer ISR preempts the thread.
/// - `Running` → `Sleeping`: the thread voluntarily waits (e.g., for a key).
/// - `Running` → `Blocked`: the thread is waiting to acquire a [`crate::sync::sync::Mutex`].
/// - Any → `Zombie`: the thread has called `exit_thread` and is awaiting cleanup.
#[derive(Debug, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Ready,
    Sleeping,
    Blocked,
    Zombie,
}

pub const MAX_FDS: usize = 1024;

/// A kernel process — a container for a page table (address space).
///
/// Currently each process holds exactly one page table (CR3 value).  Threads
/// are associated with a process via their `pid` field.  On a context switch
/// between threads belonging to different processes the scheduler reloads CR3.
pub struct Process {
    pub pid: u64,
    pub page_table: u64,
    pub heap_start: u64,
    pub program_break: u64,
    pub fd_table: [Option<alloc::sync::Arc<spin::Mutex<crate::fs::vfs::OpenFile>>>; MAX_FDS],
}

/// A kernel thread — the schedulable unit of execution.
///
/// Each `Thread` owns:
/// - A kernel stack (`stack: Vec<u8>`) allocated from the heap.
/// - A saved stack pointer (`stack_pointer`) pointing to the top of its saved
///   [`ThreadContext`] on that stack.
/// - EEVDF scheduling metadata: `weight`, `real_runtime`, `vruntime`, `lag`,
///   `time_slice`, and `deadline`.
/// - A BORE burst score for penalising CPU-hungry threads.
/// - A raw pointer to its [`SchedNode`] in the red-black scheduler tree.
/// - A [`crate::sync::sync::WaitQueue`] (`join_queue`) for threads waiting on
///   this thread to exit.
pub struct Thread {
    pub tid: u64,
    pub pid: u64,
    pub stack_pointer: u64, // The physical location of the RSP register
    pub state: ThreadState, // e.g., Running, Ready, Sleeping
    pub page_table: u64,    // The CR3 register (for when we add User Space later)
    pub stack: alloc::vec::Vec<u8>,

    // Stuff for EEVDF Scheduler
    // EEVDF -> Earliest Eligible Deadline First
    pub weight: u64,       // Priority (Base 1024)
    pub real_runtime: u64, // Total execution time
    pub vruntime: u64,     // Virtual runtime
    pub lag: i64,          // Lag acquired by the task (can be negative)
    pub time_slice: u64,   // Assigned time slice for the task
    pub deadline: u64,     // Virtual Deadline

    // BORE (Burst Oriented Response Enhancement) metric
    pub burst_score: u64,

    pub rb_node: *mut SchedNode,
    pub next_waiter: Option<usize>,

    pub join_queue: crate::sync::sync::WaitQueue,
}

/// A slot in the [`ThreadArena`] that is either occupied by a live thread or
/// linked into the arena's free list.
pub enum ThreadSlot {
    Empty { next_free: Option<usize> },
    Occupied(Thread),
}

/// A slab-style arena for kernel threads with O(1) insert, remove, and lookup.
///
/// Backed by a `Vec<ThreadSlot>`, the arena reuses dead thread slots via a
/// free list embedded in the `Empty` variant, so slot indices (thread IDs)
/// remain stable across insertions and removals — no shifting or re-indexing.
pub struct ThreadArena {
    slots: alloc::vec::Vec<ThreadSlot>,
    free_head: Option<usize>,
}

impl ThreadArena {
    pub const fn new() -> Self {
        Self {
            slots: alloc::vec::Vec::new(), // We still use Vec as the backing storage
            free_head: None,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Inserts a task and returns its permanent, non-shifting ID
    pub fn insert(&mut self, task: Thread) -> usize {
        if let Some(idx) = self.free_head {
            // Fast Path: Reuse a dead task's slot in O(1) time
            if let ThreadSlot::Empty { next_free } = self.slots[idx] {
                self.free_head = next_free;
                self.slots[idx] = ThreadSlot::Occupied(task);
                return idx;
            } else {
                panic!("FATAL: ThreadArena free list corrupted");
            }
        } else {
            // Slow Path: Array is full, push a new slot
            let idx = self.slots.len();
            self.slots.push(ThreadSlot::Occupied(task));
            idx
        }
    }

    /// Removes a task, adding its slot to the free-list
    pub fn remove(&mut self, pid: usize) -> Option<Thread> {
        if pid >= self.slots.len() {
            return None;
        }

        // Swap out the task, replacing it with an Empty slot pointing to the current free_head
        let slot = core::mem::replace(
            &mut self.slots[pid],
            ThreadSlot::Empty {
                next_free: self.free_head,
            },
        );

        match slot {
            ThreadSlot::Occupied(task) => {
                self.free_head = Some(pid); // Wire the free-list to this newly freed slot
                Some(task)
            }
            ThreadSlot::Empty { .. } => {
                // It was already empty. Revert the swap and return None.
                self.slots[pid] = slot;
                None
            }
        }
    }

    /// Safely fetch a mutable reference to a task by ID
    pub fn get_mut(&mut self, pid: usize) -> Option<&mut Thread> {
        if let Some(ThreadSlot::Occupied(task)) = self.slots.get_mut(pid) {
            Some(task)
        } else {
            None
        }
    }
}

/// Marks the current thread as a zombie, wakes all threads joining it, and yields.
///
/// Called when a thread function returns (via the trampoline `exit_thread`
/// pointer written at the bottom of every thread stack).  After marking the
/// state `Zombie`, it wakes all threads sleeping in the thread's `join_queue`,
/// drops the scheduler lock, and triggers a context switch via `int 0x20`.
pub fn exit_thread() -> ! {
    let mut sched = SCHEDULER.lock();
    let tid = sched
        .current_task
        .expect("FATAL: Phantome thread called exit");
    sched.tasks.get_mut(tid).unwrap().state = ThreadState::Zombie;
    let queue_ptr: *const crate::sync::sync::WaitQueue = {
        let task = sched.tasks.get_mut(tid).unwrap();
        &task.join_queue as *const _
    };

    unsafe {
        (*queue_ptr).wake_all(&mut *sched);
    }

    drop(sched);
    unsafe {
        core::arch::asm!("int 0x20");
    }

    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// The idle task — runs when no runnable thread exists.
///
/// Issues `hlt` in a loop, putting the CPU into a low-power halted state
/// until the next interrupt.  The scheduler resumes a real thread as soon as
/// one becomes runnable.
///
/// # Safety
/// Declared `extern "C"` so it can be used as a raw function pointer.
pub unsafe extern "C" fn idle_task() {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

/// The global EEVDF + BORE scheduler instance.
///
/// All scheduling operations (task insertion, context-switch decisions, task
/// wakeups) are performed through this singleton.  It is protected by
/// [`crate::mm::allocator::Locked`] so that both normal kernel code and the
/// timer ISR can safely access it with interrupt-safe locking.
pub struct Scheduler {
    pub processes: alloc::vec::Vec<Option<Process>>,
    pub tasks: ThreadArena,
    pub total_weight: u64,
    pub system_runtime: u64, // Global virtual clock
    pub current_task: Option<usize>,
    pub tree_root: *mut SchedNode,
    pub idle_task_id: Option<usize>,
    pub graveyard: alloc::collections::VecDeque<usize>, // Dead tasks
}

pub static SCHEDULER: crate::mm::allocator::Locked<Scheduler> =
    crate::mm::allocator::Locked::new(Scheduler::new());
unsafe impl Send for Scheduler {}
impl Scheduler {
    pub const fn new() -> Self {
        Self {
            processes: alloc::vec::Vec::new(),
            tasks: ThreadArena::new(),
            total_weight: 0,
            system_runtime: 0,
            current_task: None,
            tree_root: core::ptr::null_mut(),
            idle_task_id: None,
            graveyard: alloc::collections::VecDeque::new(),
        }
    }

    pub fn reap_zombies(&mut self) {
        let len = self.graveyard.len();
        for _i in 0..len {
            let zombie_id = self.graveyard.pop_front();
            if zombie_id != self.current_task {
                self.tasks.remove(zombie_id.unwrap());
            } else {
                self.graveyard.push_back(zombie_id.unwrap());
            }
        }
    }

    pub fn add_task(&mut self, task: Thread) -> usize {
        self.total_weight += task.weight;
        let vruntime = task.vruntime;
        let deadline = task.deadline;

        let task_id = self.tasks.insert(task) as u64;
        self.tasks.get_mut(task_id as usize).unwrap().tid = task_id;

        // Allocate the C-compatible struct on the Rust heap
        let new_node = alloc::boxed::Box::new(SchedNode {
            vruntime,
            task_id,
            deadline: deadline,
            min_deadline: deadline,
            left: core::ptr::null_mut(),
            right: core::ptr::null_mut(),
            parent_and_color: 0,
        });

        // Strip away Rust's ownership to get a raw C pointer
        let raw_node_ptr = alloc::boxed::Box::into_raw(new_node);
        self.tasks.get_mut(task_id as usize).unwrap().rb_node = raw_node_ptr;

        // Pass it to  C code!
        unsafe {
            rbtree_insert(&mut self.tree_root, raw_node_ptr);
        }

        task_id as usize
    }

    pub fn schedule(&mut self, context: &mut ThreadContext) -> *mut ThreadContext {
        // Our first task
        if self.tasks.is_empty() {
            return context as *mut ThreadContext;
        }

        self.reap_zombies();

        // We know the APIC timer ticks exactly once per millisecond
        let time_consumed: u64 = 1_000_000; // 1ms in nanoseconds

        // If there is a current_task running...
        if let Some(task_idx) = self.current_task {
            if let Some(task) = self.tasks.get_mut(task_idx) {
                if task.state == ThreadState::Zombie {
                    let raw_node = task.rb_node;
                    if !raw_node.is_null() {
                        // Re-box it and let it immediately go out of scope to free the heap memory!
                        let _ = unsafe { alloc::boxed::Box::from_raw(raw_node) };
                    }
                    self.graveyard.push_back(task_idx);
                    task.rb_node = core::ptr::null_mut();
                } else {
                    // Save its hardware context
                    task.stack_pointer = context as *mut _ as u64;

                    // Update Real Runtime
                    task.real_runtime += time_consumed;

                    // Update BORE BURST SCORE
                    task.burst_score = (task.burst_score + time_consumed) >> 1;

                    // Update System Virtual Time (V)
                    self.system_runtime += (time_consumed << 20) / self.total_weight;

                    // Update Thread Deadline
                    task.update_deadline();

                    // Update Thread Lag
                    task.lag = self.system_runtime as i64 - task.vruntime as i64;
                }

                if task.state == ThreadState::Ready
                    && task.tid as usize != self.idle_task_id.unwrap()
                {
                    unsafe {
                        (*task.rb_node).vruntime = task.vruntime;
                        (*task.rb_node).deadline = task.deadline;
                        rbtree_insert(&mut self.tree_root, task.rb_node);
                    }
                }
            }
        }

        //  Ask the C tree for the node with the lowest vruntime
        let leftmost_node = unsafe { rbtree_pick_eevdf(self.tree_root) };

        if leftmost_node.is_null() {
            if let Some(idle_task_id) = self.idle_task_id {
                // Update current_task so we don't accidentally save the idle task's context over a real task next tick
                self.current_task = Some(idle_task_id);
                let task = self.tasks.get_mut(idle_task_id).unwrap();
                unsafe {
                    crate::arch::x86_64::cpu::PER_CPU_0.kernel_rsp =
                        task.stack.as_ptr() as u64 + task.stack.len() as u64;
                }
                return task.stack_pointer as *mut ThreadContext;
            } else {
                panic!("FATAL: Scheduling tree is empty and no idle task is set!");
            }
        }

        unsafe {
            rbtree_remove(&mut self.tree_root, leftmost_node);
        }

        //  Safely read the task_id out of the C struct
        let winner_idx = unsafe { (*leftmost_node).task_id as usize };

        let prev_pid = if let Some(prev_idx) = self.current_task {
            self.tasks.get_mut(prev_idx).unwrap().pid as usize
        } else {
            usize::MAX
        };

        self.current_task = Some(winner_idx);

        let next_pid = self.tasks.get_mut(winner_idx).unwrap().pid;

        if prev_pid as u64 != next_pid {
            let slot: &Option<Process> = &self.processes[next_pid as usize];
            let process: &Process = slot.as_ref().expect("...");

            let phys_addr = process.page_table;
            unsafe {
                use x86_64::PhysAddr;
                use x86_64::registers::control::{Cr3, Cr3Flags};
                use x86_64::structures::paging::PhysFrame;

                let frame = PhysFrame::containing_address(PhysAddr::new(phys_addr));
                Cr3::write(frame, Cr3Flags::empty());
            }
        }

        // Fetch the hardware context from our Rust ThreadArena
        if let Some(task) = self.tasks.get_mut(winner_idx) {
            unsafe {
                let stack_top = task.stack.as_ptr() as u64 + task.stack.len() as u64;
                crate::arch::x86_64::cpu::PER_CPU_0.kernel_rsp = stack_top;
                crate::arch::x86_64::gdt::set_tss_rsp0(x86_64::VirtAddr::new(stack_top));
            }

            return task.stack_pointer as *mut ThreadContext;
        } else {
            // If the tree points to a dead/missing ID, we have a severe synchronization bug
            panic!(
                "FATAL: RB-Tree returned task_id {} which is missing from the Arena!",
                winner_idx
            );
        }
    }

    pub fn set_idle_task(&mut self, task: Thread) {
        let task_id = self.tasks.insert(task) as u64;
        self.tasks.get_mut(task_id as usize).unwrap().tid = task_id;
        self.idle_task_id = Some(task_id as usize);
    }

    pub fn sleep_current_task(&mut self) {
        if let Some(task_idx) = self.current_task {
            self.tasks.get_mut(task_idx).unwrap().state = ThreadState::Sleeping;
        }
    }

    pub fn wake_task(&mut self, task_idx: usize) {
        if let Some(task) = self.tasks.get_mut(task_idx) {
            if task.state == ThreadState::Sleeping || task.state == ThreadState::Blocked {
                task.state = ThreadState::Ready;

                let min_vruntime = self.system_runtime.saturating_sub(task.time_slice);
                if task.vruntime < min_vruntime {
                    task.vruntime = min_vruntime;
                }

                task.burst_score = 0;
                task.update_deadline();

                if self.current_task != Some(task_idx) {
                    unsafe {
                        (*task.rb_node).vruntime = task.vruntime;
                        (*task.rb_node).deadline = task.deadline;
                        rbtree_insert(&mut self.tree_root, task.rb_node);
                    }
                }
            }
        }
    }
}

const TASK_STACK_SIZE: usize = 0x400 * 16; // 16 KB
const SCHEDULER_TARGET_LATENCY: u64 = 6 * 1_000_000; // 6 ms Defaul in Linux
const SCHEDULER_MIN_GRANULARITY: u64 = 4 * 1_000_000; // 4 ms Default in Linux
const NICE_0_LOAD: u64 = 1024; // Value base Nice

impl Thread {
    pub fn new(scheduler: &mut Scheduler, entry_point: u64, weight: u64) -> Self {
        // Allocte memory for the stack
        let mut stack = alloc::vec![0; TASK_STACK_SIZE];

        // Get the highest stack address
        let stack_start = stack.as_mut_ptr() as u64;
        let stack_end = stack_start + TASK_STACK_SIZE as u64;

        let return_addr_ptr = stack_end - 8;

        unsafe {
            core::ptr::write(
                return_addr_ptr as *mut u64,
                crate::process::process::exit_thread as *const () as u64,
            );
        }

        // Step back 16 bytes to make room to write a 64-bit return address
        let initial_rsp = return_addr_ptr - core::mem::size_of::<ThreadContext>() as u64;

        let context = ThreadContext {
            rip: entry_point,
            cs: 0x8,
            rflags: 0x202,
            rsp: return_addr_ptr,
            ss: 0x0,
            ..Default::default()
        };

        // Write context at the top of the TASK_STACK_SIZE
        unsafe {
            core::ptr::write(initial_rsp as *mut ThreadContext, context);
        }

        let vruntime: u64 = 0;

        let time_slice = if scheduler.total_weight > 0 {
            core::cmp::max(
                SCHEDULER_MIN_GRANULARITY,
                (weight * SCHEDULER_TARGET_LATENCY) / scheduler.total_weight,
            )
        } else {
            core::cmp::max(
                SCHEDULER_MIN_GRANULARITY,
                (weight * SCHEDULER_TARGET_LATENCY) / weight,
            )
        };

        let lag: i64 = scheduler.system_runtime as i64 - vruntime as i64;

        let deadline = vruntime + ((time_slice * NICE_0_LOAD) << 20) / weight;

        Self {
            tid: 0,
            pid: 0,
            stack_pointer: initial_rsp,
            state: ThreadState::Ready,
            page_table: 0,
            stack,
            weight,
            real_runtime: 0,
            vruntime,
            lag,
            time_slice,
            deadline,
            burst_score: 0,
            rb_node: core::ptr::null_mut(),
            next_waiter: None,
            join_queue: crate::sync::sync::WaitQueue::new(),
        }
    }

    pub fn set_priority(&mut self, priority: u64) {
        self.weight = priority;
    }

    pub fn effective_weight(&self) -> u64 {
        // How many 4ms "penalties" has this task accumulated?
        let penalty_points = self.burst_score / SCHEDULER_MIN_GRANULARITY;

        // Shift the base weight down by the penalty points.
        // max(1, ...) ensures we never divide by zero later.
        core::cmp::max(1, self.weight >> penalty_points)
    }

    pub fn update_deadline(&mut self) {
        let eff_weight = self.effective_weight();
        self.vruntime = (self.real_runtime << 20) / eff_weight;
        let virtual_slice = (self.time_slice << 20) / eff_weight;
        self.deadline = self.vruntime + virtual_slice;
    }
}

pub static SYSTEM_UPTIME_NANOS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(0);
pub fn update_system_uptime() {
    // Since our timer ticks at 1000 Hz, we need to add 1,000,000 ns every tick
    SYSTEM_UPTIME_NANOS.fetch_add(1_000_000, core::sync::atomic::Ordering::Relaxed);
}

/// Timer interrupt callback — updates the system clock and triggers a context switch.
///
/// Called from the `timer_isr` assembly stub (which saved the full
/// [`ThreadContext`]) with `context` pointing at the saved register state on
/// the current thread's kernel stack.  Returns a pointer to the `ThreadContext`
/// of the next thread to run; the assembly stub restores that context and
/// executes `iretq`.
///
/// # `#[unsafe(no_mangle)]`
/// Required so the assembly stub can call this Rust function by name.
#[unsafe(no_mangle)]
pub extern "C" fn rust_timer_handler(
    context: &mut crate::process::process::ThreadContext,
) -> *mut ThreadContext {
    // Update the system uptime
    update_system_uptime();
    // Tell the APIC we received the interrupt
    crate::interrupts::apic::LOCAL_APIC
        .lock()
        .as_ref()
        .unwrap()
        .end_of_interrupt();

    // Call the scheduler!
    let next_context = SCHEDULER.lock().schedule(context);

    // crate::serial_println!("Timer tick! Context is at: {:p}", next_context);
    return next_context;
}

/// Immediately yields the CPU to the scheduler by triggering the timer interrupt vector.
///
/// Issues `int 0x20` (the APIC timer vector) directly.  The timer ISR then
/// calls `rust_timer_handler`, which runs the EEVDF scheduler and switches to
/// whichever thread has the earliest virtual deadline.
// achieves the exact same context switch instantly!
pub fn yield_now() {
    unsafe {
        core::arch::asm!("int 0x20");
    }
}

/// Spawns a new kernel thread at `entry_point` with the given scheduling `weight`.
///
/// Allocates a [`Thread`] (including its kernel stack), adds it to the global
/// [`SCHEDULER`]'s red-black tree, and returns the new thread ID.
// Spawns a new thread and returns its ID
pub fn spawn(entry_point: extern "C" fn(), weight: u64) -> usize {
    let mut sched = SCHEDULER.lock();
    let thread = Thread::new(&mut sched, entry_point as u64, weight);
    sched.add_task(thread)
}

/// Blocks the current thread until thread `target_tid` exits.
///
/// Adds the current thread's TID to `target`'s `join_queue`, marks the
/// current thread as `Blocked`, drops all locks, and yields.  When the target
/// thread calls `exit_thread`, it calls `wake_all` on its `join_queue`,
/// which moves all joined threads back to `Ready`.
///
/// Returns immediately if `target_tid` is already `Zombie` or if the caller
/// tries to join itself (deadlock prevention).
pub fn join(target_tid: usize) {
    let mut sched = SCHEDULER.lock();

    let current_id = sched.current_task.expect("FATAL: No current task!");
    if current_id == target_tid {
        return; // Deadlock prevention: you cannot join yourself.
    }

    let queue_ptr: *const crate::sync::sync::WaitQueue = {
        if let Some(target_thread) = sched.tasks.get_mut(target_tid) {
            if target_thread.state == ThreadState::Zombie {
                return;
            }

            &target_thread.join_queue as *const _
        } else {
            return;
        }
    };

    x86_64::instructions::interrupts::disable();

    let current_task = sched.tasks.get_mut(current_id).unwrap();
    current_task.state = crate::process::process::ThreadState::Blocked;

    unsafe {
        let mut state = (*queue_ptr).state.lock();
        state.waiters.push_back(current_id);
        drop(state);
    }

    drop(sched);
    x86_64::instructions::interrupts::enable();
    unsafe {
        core::arch::asm!("int 0x20");
    }
}

// src/process/process.rs

/// Spawns a user-mode process in its own isolated address space.
pub fn spawn_user_process(elf_data: &[u8], weight: u64) -> usize {
    let elf = xmas_elf::ElfFile::new(elf_data).expect("Failed to parse ELF");
    let entry_point = elf.header.pt2.entry_point();

    let user_pml4 = crate::mm::paging::create_user_address_space().unwrap();

    let virt_addr = x86_64::VirtAddr::new(user_pml4.as_u64() + crate::mm::paging::PHYS_OFFSET);
    let user_pml4_table =
        unsafe { &mut *(virt_addr.as_mut_ptr() as *mut x86_64::structures::paging::PageTable) };

    let code_frame = crate::mm::memory::allocate_frame().expect("OOM");

    crate::mm::paging::map_to(
        x86_64::structures::paging::Page::containing_address(x86_64::VirtAddr::new(0x4000_0000)),
        x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(
            code_frame as u64,
        )),
        x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE,
        user_pml4_table,
    )
    .expect("OOM");

    let mut highest_vaddr: u64 = 0;

    for ph in elf.program_iter() {
        if let Ok(xmas_elf::program::Type::Load) = ph.get_type() {
            let vaddr = ph.virtual_addr();
            let mem_size = ph.mem_size();
            let file_size = ph.file_size();
            let offset = ph.offset();

            let start_page = vaddr / 4096;
            let end_page = (vaddr + mem_size - 1) / 4096;

            let mut current_file_offset = offset;
            let mut remaining_file_bytes = file_size;
            let mut current_vaddr = vaddr;

            highest_vaddr = core::cmp::max(highest_vaddr, vaddr + mem_size);

            for page_num in start_page..=end_page {
                let page_vaddr = page_num * 4096;

                let frame = crate::mm::memory::allocate_zeroed_frame().expect("OOM");

                crate::mm::paging::map_to(
                    x86_64::structures::paging::Page::containing_address(x86_64::VirtAddr::new(
                        page_vaddr,
                    )),
                    x86_64::structures::paging::PhysFrame::containing_address(
                        x86_64::PhysAddr::new(frame as u64),
                    ),
                    x86_64::structures::paging::PageTableFlags::PRESENT
                        | x86_64::structures::paging::PageTableFlags::WRITABLE
                        | x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE,
                    user_pml4_table,
                )
                .expect("OOM");

                let page_offset = if current_vaddr > page_vaddr {
                    current_vaddr - page_vaddr
                } else {
                    0
                };
                let bytes_to_copy = core::cmp::min(remaining_file_bytes, 4096 - page_offset);

                if bytes_to_copy > 0 {
                    // Get the virtual pointer to the physical frame via PHYS_OFFSET
                    let dest_ptr =
                        (frame as u64 + crate::mm::paging::PHYS_OFFSET + page_offset) as *mut u8;
                    let src_ptr = unsafe { elf_data.as_ptr().add(current_file_offset as usize) };

                    unsafe {
                        core::ptr::copy_nonoverlapping(src_ptr, dest_ptr, bytes_to_copy as usize);
                    }

                    remaining_file_bytes -= bytes_to_copy;
                    current_file_offset += bytes_to_copy;
                    current_vaddr += bytes_to_copy;
                }
            }
        }
    }

    let stack_frame = crate::mm::memory::allocate_frame().expect("OOM");

    crate::mm::paging::map_to(
        x86_64::structures::paging::Page::containing_address(x86_64::VirtAddr::new(0x8000_0000)),
        x86_64::structures::paging::PhysFrame::containing_address(x86_64::PhysAddr::new(
            stack_frame as u64,
        )),
        x86_64::structures::paging::PageTableFlags::PRESENT
            | x86_64::structures::paging::PageTableFlags::WRITABLE
            | x86_64::structures::paging::PageTableFlags::USER_ACCESSIBLE
            | x86_64::structures::paging::PageTableFlags::NO_EXECUTE,
        user_pml4_table,
    )
    .expect("OOM");

    let initial_heap_start = crate::mm::allocator::align_to(highest_vaddr as usize, 4096);

    let console_vnode = alloc::sync::Arc::new(crate::fs::vfs::ConsoleVnode);

    let console_file = alloc::sync::Arc::new(spin::Mutex::new(crate::fs::vfs::OpenFile {
        vnode: console_vnode,
        offset: 0,
        readable: true,
        writable: true,
    }));

    let mut fd_table: [Option<alloc::sync::Arc<spin::Mutex<crate::fs::vfs::OpenFile>>>; MAX_FDS] =
        [const { None }; MAX_FDS];

    fd_table[0] = Some(console_file.clone());
    fd_table[1] = Some(console_file.clone());
    fd_table[2] = Some(console_file.clone());

    let mut sched = SCHEDULER.lock();

    let pid = sched.processes.len() as u64;
    let process = Process {
        pid,
        page_table: user_pml4.as_u64(),
        heap_start: initial_heap_start as u64,
        program_break: initial_heap_start as u64,
        fd_table: fd_table,
    };

    sched.processes.push(Some(process));

    let mut thread =
        crate::process::process::Thread::new(&mut sched, user_mode_trampoline as u64, weight);

    unsafe {
        let context_ptr = thread.stack_pointer as *mut crate::process::process::ThreadContext;
        (*context_ptr).rdi = entry_point;
    }

    thread.pid = pid;

    let tid = sched.add_task(thread);
    return tid;
}

/// A tiny kernel thread that drops privileges and jumps to user space.
pub extern "C" fn user_mode_trampoline(enty_point: u64) {
    let stack_top = 0x8000_0000 + 4096;

    unsafe {
        crate::arch::x86_64::gdt::jump_to_user_space(enty_point, stack_top);
    }
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

#[repr(C)]
pub struct SchedNode {
    pub vruntime: u64,
    pub task_id: u64,
    pub deadline: u64,
    pub min_deadline: u64,
    pub left: *mut SchedNode,
    pub right: *mut SchedNode,
    pub parent_and_color: usize,
}

// Define the FFI bindings
unsafe extern "C" {
    pub fn rbtree_insert(root: *mut *mut SchedNode, new_node: *mut SchedNode);
    pub fn rbtree_pick_eevdf(root: *mut SchedNode) -> *mut SchedNode;
    pub fn rbtree_remove(root: *mut *mut SchedNode, node: *mut SchedNode);
}

// Test tasks

use crate::sync::sync::Mutex;

pub static TEST_MUTEX: Mutex<usize> = Mutex::new(0);

pub extern "C" fn task_a() {
    loop {
        crate::serial_println!("Thread A going to sleep waiting for keypress...");

        let (code, stack) = crate::mm::paging::setup_user_sandbox();
        unsafe {
            crate::arch::x86_64::gdt::jump_to_user_space(code, stack);
        }

        // if let Some(key) = crate::drivers::keyboard::KEYBOARD_MAILBOX.receive() {
        //     match key {
        //         pc_keyboard::DecodedKey::Unicode(character) => {
        //             crate::print!("{}", character);
        //             crate::serial_print!("{}", character);
        //         }
        //         pc_keyboard::DecodedKey::RawKey(_key) => continue,
        //     }
        // }
    }
}

pub extern "C" fn task_b() {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
