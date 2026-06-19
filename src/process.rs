// src/process.rs

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

#[derive(Debug, PartialEq, Eq)]
pub enum ThreadState {
    Running,
    Ready,
    Sleeping,
    Blocked,
}

pub struct Process {
    pub pid: u64,
    pub page_table: u64,
}

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
}

pub enum ThreadSlot {
    Empty { next_free: Option<usize> },
    Occupied(Thread),
}

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

pub unsafe extern "C" fn idle_task() {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}

pub struct Scheduler {
    pub processes: alloc::vec::Vec<Option<Process>>,
    pub tasks: ThreadArena,
    pub total_weight: u64,
    pub system_runtime: u64, // Global virtual clock
    pub current_task: Option<usize>,
    pub tree_root: *mut SchedNode,
    pub idle_task_id: Option<usize>,
}

pub static SCHEDULER: crate::allocator::Locked<Scheduler> =
    crate::allocator::Locked::new(Scheduler::new());
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
        }
    }
    pub fn add_task(&mut self, task: Thread) {
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
    }

    pub fn schedule(&mut self, context: &mut ThreadContext) -> *mut ThreadContext {
        // Our first task
        if self.tasks.is_empty() {
            return context as *mut ThreadContext;
        }

        // We know the APIC timer ticks exactly once per millisecond
        let time_consumed: u64 = 1_000_000; // 1ms in nanoseconds

        // If there is a current_task running...
        if let Some(task_idx) = self.current_task {
            if let Some(task) = self.tasks.get_mut(task_idx) {
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
                return self.tasks.get_mut(idle_task_id).unwrap().stack_pointer
                    as *mut ThreadContext;
            } else {
                panic!("FATAL: Scheduling tree is empty and no idle task is set!");
            }
        }

        unsafe {
            rbtree_remove(&mut self.tree_root, leftmost_node);
        }

        //  Safely read the task_id out of the C struct
        let winner_idx = unsafe { (*leftmost_node).task_id as usize };

        self.current_task = Some(winner_idx);

        let next_pid = self.tasks.get_mut(winner_idx).unwrap().pid;

        let prev_pid = if let Some(prev_idx) = self.current_task {
            self.tasks.get_mut(prev_idx).unwrap().pid as usize
        } else {
            usize::MAX
        };

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

        // Step back 16 bytes to make room to write a 64-bit return address
        let initial_rsp = stack_end - core::mem::size_of::<ThreadContext>() as u64;

        let context = ThreadContext {
            rip: entry_point,
            cs: 0x8,
            rflags: 0x202,
            rsp: initial_rsp,
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

#[unsafe(no_mangle)]
pub extern "C" fn rust_timer_handler(
    context: &mut crate::process::ThreadContext,
) -> *mut ThreadContext {
    // Update the system uptime
    update_system_uptime();
    // Tell the APIC we received the interrupt
    crate::apic::LOCAL_APIC
        .lock()
        .as_ref()
        .unwrap()
        .end_of_interrupt();

    // Call the scheduler!
    let next_context = SCHEDULER.lock().schedule(context);

    // crate::serial_println!("Timer tick! Context is at: {:p}", next_context);
    return next_context;
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

use crate::sync::Mutex;

pub static TEST_MUTEX: Mutex<usize> = Mutex::new(0);

pub extern "C" fn task_a() {
    loop {
        crate::println!("Thread A going to sleep waiting for keypress...");
        crate::interrupt::KEYBOARD_SEMAPHORE.acquire();

        if let Some(key) = crate::keyboard::KEYBOARD_EVENTS.pop() {
            crate::println!("Thread A woke up and received key: {:?}", key);
        }
    }
}

pub extern "C" fn task_b() {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
