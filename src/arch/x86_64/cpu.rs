// src/cpu.rs

/// Per-CPU data block pinned to the `GS` segment base register.
///
/// One `PerCpu` is statically allocated for each logical CPU. The layout is
/// `#[repr(C)]` so that inline assembly in the syscall stub can reference its
/// fields by **known, fixed byte offsets** relative to the GS base:
///
/// | Field | Offset | Purpose |
/// |-------|--------|---------|
/// | `cpu_id` | 0 | Logical CPU index (0-based) |
/// | `apic_id` | 4 | Hardware APIC ID read from the LAPIC |
/// | `kernel_rsp` | 8 | Saved kernel-stack pointer – loaded on every `syscall`/`int` entry |
/// | `user_rsp_scratch` | 16 | Temporary scratch slot for the user RSP during `syscall` entry |
/// | `current_task_ptr` | 24 | Raw pointer to the currently executing `Thread` |
///
/// # Safety
/// All fields are accessed from inline assembly using GS-relative addressing.
/// Do **not** reorder or add fields without updating the assembly offsets in
/// `src/process/syscall.rs` and `src/interrupts/interrupt.rs`.
#[repr(C)]
pub struct PerCpu {
    pub cpu_id: u32,                                            // Offset 0
    pub apic_id: u32,                                           // Offset 4
    pub kernel_rsp: u64,       // Offset 8  <-- We will load this into RSP
    pub user_rsp_scratch: u64, // Offset 16 <-- We will save the user's RSP here
    pub current_task_ptr: *mut crate::process::process::Thread, // Offset 24
}

/// The single statically-allocated `PerCpu` block for logical CPU 0 (the BSP).
///
/// Because this kernel currently only runs on a single core, there is exactly
/// one of these. On a multi-core system you would allocate one per core, then
/// point each CPU's `GS` base at its own block.
///
/// # Safety
/// This is a `static mut` intentionally. It is exclusively written during
/// single-threaded early-boot (before interrupts or other CPUs are active) and
/// then read-only except for the fields that are updated from assembly stubs
/// under controlled conditions.
pub static mut PER_CPU_0: PerCpu = PerCpu {
    cpu_id: 0,
    apic_id: 0,
    kernel_rsp: 0,
    user_rsp_scratch: 0,
    current_task_ptr: core::ptr::null_mut(),
};

/// Initialises the GS-base MSR to point at [`PER_CPU_0`].
///
/// The x86_64 SYSCALL/SYSRET ABI uses the `GS` segment register to locate
/// per-CPU data. This function:
/// 1. Takes the virtual address of the static `PER_CPU_0` struct.
/// 2. Writes it to the `IA32_GS_BASE` MSR via the `x86_64` crate helper.
/// 3. Reads it back and prints it on the serial console to confirm success.
///
/// **Must be called before `syscall::init()`** so that the SYSCALL entry stub
/// can immediately use `swapgs` + GS-relative addressing.
pub fn init_cpu_local() {
    let ptr = &raw const PER_CPU_0 as u64;

    let virt_addr = x86_64::VirtAddr::new(ptr);

    x86_64::registers::model_specific::GsBase::write(virt_addr);

    let gs_base = x86_64::registers::model_specific::GsBase::read();
    crate::serial_println!("GS Base: {:#x}", gs_base.as_u64());
}


pub unsafe fn enable_nx_bit() {
    use x86_64::registers::model_specific::{Efer, EferFlags};
    let mut efer = Efer::read();
    if !efer.contains(EferFlags::NO_EXECUTE_ENABLE) {
        efer |= EferFlags::NO_EXECUTE_ENABLE;
        unsafe {
            Efer::write(efer);
        }
    }
}
