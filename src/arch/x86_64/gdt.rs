// src/arch/x86_64/gdt.rs

use spin::Once;
use x86_64::VirtAddr;
use x86_64::structures::gdt::SegmentSelector;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable};
use x86_64::structures::tss::TaskStateSegment;

/// IST (Interrupt Stack Table) slot index used for the double-fault handler.
///
/// The x86_64 TSS provides 7 IST slots (0–6), each pointing to an independent
/// stack. By wiring the double-fault IDT entry to slot 0, the CPU will
/// automatically switch to a known-good stack before invoking the handler,
/// preventing a triple-fault when the kernel stack itself is corrupted.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Size (in bytes) of each dedicated exception stack (64 KiB).
const STACK_SIZE: usize = 4096 * 16;

/// A 16-byte-aligned raw byte array used as a dedicated CPU exception stack.
///
/// The `#[repr(align(16))]` attribute is mandatory: the x86_64 ABI requires
/// that the stack pointer is 16-byte aligned before any `call` instruction, so
/// starting with a misaligned base would immediately violate the ABI.
// Force the compiler to 16-byte align the stack array
#[repr(align(16))]
pub struct Stack {
    buffer: [u8; STACK_SIZE],
}

/// Dedicated stack used exclusively by the double-fault handler (IST slot 0).
///
/// Having a separate stack means a double fault that occurs because the main
/// kernel stack overflowed will still be handled correctly — the handler runs
/// on this pristine 64 KiB region rather than continuing to corrupt memory.
static DOUBLE_FAULT_STACK: Stack = Stack {
    buffer: [0; STACK_SIZE],
};

/// Kernel stack automatically loaded by the hardware on Ring 3 → Ring 0 transitions.
///
/// When the CPU executes a `syscall` instruction from user mode (Ring 3) it
/// does **not** automatically switch stacks. However, when an interrupt fires
/// while the CPU is at Ring 3, the TSS `privilege_stack_table[0]` entry is
/// used as the new RSP. This stack is also used by `iretq` when returning from
/// a kernel-mode interrupt to ensure we have a clean kernel stack.
static PRIVILEGE_STACK: Stack = Stack {
    buffer: [0; STACK_SIZE],
};

/// Newtype wrapper around [`TaskStateSegment`] with mandatory 16-byte alignment.
///
/// Some CPU implementations require the TSS structure itself to be aligned;
/// the wrapper guarantees this regardless of how the linker places the static.
// Force the compiler to 16-byte align the TSS
#[repr(align(16))]
struct AlignedTss(TaskStateSegment);

/// Holds all segment selectors produced when building the GDT.
///
/// After [`GlobalDescriptorTable::load`] is called the selectors can be loaded
/// into the segment registers. We keep them in this bundle so that helper
/// functions like [`kernel_code_selector`] can retrieve them cheaply after
/// boot without re-parsing the table.
struct Gdt {
    table: GlobalDescriptorTable,
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    user_data: SegmentSelector,
    user_code: SegmentSelector,
}

/// Lazily-initialised, globally-shared TSS.  Initialised exactly once in [`init`].
static TSS_ONCE: Once<AlignedTss> = Once::new();

/// Lazily-initialised, globally-shared GDT bundle.  Initialised exactly once in [`init`].
static GDT_ONCE: Once<Gdt> = Once::new();

/// Returns the Ring-3 data segment selector for use when constructing user-space frames.
///
/// Panics if the GDT has not yet been initialised via [`init`].
pub fn user_data_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").user_data
}

/// Returns the Ring-0 (kernel) code segment selector.
///
/// Used when reloading `CS` after loading the GDT and by the STAR MSR setup in
/// the syscall subsystem.  Panics if the GDT has not been initialised.
pub fn kernel_code_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").code_selector
}

/// Returns the Ring-0 (kernel) data segment selector.
///
/// Loaded into `DS`, `ES`, and `SS` during GDT initialisation and referenced
/// by the syscall STAR MSR.  Panics if the GDT has not been initialised.
pub fn kernel_data_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").data_selector
}

/// Returns the Ring-3 (user) code segment selector.
///
/// Used to build the `iretq` frame in [`jump_to_user_space`] and stored in the
/// STAR MSR so the hardware knows which selector to restore on `sysretq`.
/// Panics if the GDT has not been initialised.
pub fn user_code_selector() -> SegmentSelector {
    GDT_ONCE.r#try().expect("GDT not initialized").user_code
}

/// Builds and loads the GDT, TSS, and all segment registers.
///
/// # What this does
///
/// 1. **TSS** – Allocates the Task State Segment (via [`TSS_ONCE`]) and fills:
///    - `IST[0]` → top of [`DOUBLE_FAULT_STACK`] (for the double-fault handler).
///    - `privilege_stack_table[0]` → top of [`PRIVILEGE_STACK`] (for Ring3→Ring0 transitions).
/// 2. **GDT** – Builds a flat-memory GDT (via [`GDT_ONCE`]) with six entries:
///    - Kernel code (Ring 0, 64-bit)
///    - Kernel data (Ring 0)
///    - Dummy user code (required to place user data at the right GDT index)
///    - User data (Ring 3)
///    - User code (Ring 3, 64-bit)
///    - TSS descriptor (128-bit system segment)
/// 3. **Loads** the GDT with `lgdt` and reloads `CS`, `DS`, `ES`, `SS`, and the TSS.
///
/// # Panics
/// Panics if called more than once (protected by [`spin::Once`]).
pub fn init() {
    use crate::serial_println;
    use x86_64::instructions::segmentation::{CS, Segment};
    use x86_64::instructions::tables::load_tss;

    let tss = TSS_ONCE.call_once(|| {
        let mut tss = TaskStateSegment::new();
        let stack_start = VirtAddr::from_ptr({ &raw const DOUBLE_FAULT_STACK.buffer });
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = stack_start + STACK_SIZE;
        let priv_stack_start = VirtAddr::from_ptr({ &raw const PRIVILEGE_STACK.buffer });
        tss.privilege_stack_table[0] = priv_stack_start + STACK_SIZE;
        serial_println!(
            "TSS IST[{}]: {:#x}",
            DOUBLE_FAULT_IST_INDEX,
            (stack_start + STACK_SIZE).as_u64()
        );
        AlignedTss(tss)
    });

    let gdt = GDT_ONCE.call_once(|| {
        let mut table = GlobalDescriptorTable::new();
        let code_selector = table.add_entry(Descriptor::kernel_code_segment());
        let data_selector = table.add_entry(Descriptor::kernel_data_segment());
        let _ = table.add_entry(Descriptor::user_code_segment());
        let user_data = table.add_entry(Descriptor::user_data_segment());
        let user_code = table.add_entry(Descriptor::user_code_segment());
        let tss_selector = table.add_entry(Descriptor::tss_segment(&tss.0));

        Gdt {
            table,
            code_selector,
            data_selector,
            tss_selector,
            user_data,
            user_code,
        }
    });

    gdt.table.load();
    unsafe {
        CS::set_reg(gdt.code_selector);
        x86_64::instructions::segmentation::DS::set_reg(gdt.data_selector);
        x86_64::instructions::segmentation::ES::set_reg(gdt.data_selector);
        x86_64::instructions::segmentation::SS::set_reg(gdt.data_selector);
        load_tss(gdt.tss_selector);
    }
    serial_println!("GDT init complete");
}

/// Performs a privilege-level transition from Ring 0 into Ring 3 at `code_addr`.
///
/// # How it works
///
/// The CPU only transitions to user mode via an `iretq` instruction that pops
/// the full interrupt frame (`RIP`, `CS`, `RFLAGS`, `RSP`, `SS`) from the stack.
/// This function fabricates that frame manually, then executes `iretq` to make
/// the CPU believe it is returning from an interrupt to user space:
///
/// 1. Pushes `SS`  = user data selector (Ring 3 data segment).
/// 2. Pushes `RSP` = `stack_addr` (the pre-allocated user stack top).
/// 3. Pushes `RFLAGS` = `0x202` (interrupts enabled, reserved bit set).
/// 4. Pushes `CS`  = user code selector (Ring 3, 64-bit).
/// 5. Pushes `RIP` = `code_addr` (first instruction to execute).
/// 6. Issues `swapgs` to install the kernel GS base before crossing the ring boundary.
/// 7. Issues `iretq` — the CPU atomically lowers CPL to 3 and jumps to `code_addr`.
///
/// # Safety
/// - `code_addr` must point to valid, mapped, executable Ring-3 memory.
/// - `stack_addr` must point to the **top** of a valid, mapped, writable Ring-3 stack page.
/// - The GDT must have been initialised before calling this function.
/// - This function **never returns**.
pub unsafe fn jump_to_user_space(code_addr: u64, stack_addr: u64) -> ! {
    let user_data = crate::arch::x86_64::gdt::user_data_selector().0 as u64;
    let user_code = crate::arch::x86_64::gdt::user_code_selector().0 as u64;

    // Define our RFLAGS
    let rflags = 0x202u64;

    // Fake the interrupt frame and return
    unsafe {
        core::arch::asm!(
            "push {ss}",
            "push {rsp}",
            "push {rflags}",
            "push {cs}",
            "push {rip}",
            "swapgs",
            "iretq",
            ss = in(reg) user_data,
            rsp = in(reg) stack_addr,
            rflags = in(reg) rflags,
            cs = in(reg) user_code,
            rip = in(reg) code_addr,
            options(noreturn)
        );
    }
}
