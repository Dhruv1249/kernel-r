// src/cpu.rs

#[repr(C)]
pub struct PerCpu {
    pub cpu_id: u32,
    pub apic_id: u32,
    pub kernel_rsp: u64, // Used later to restore the kernel stack during a syscall
    pub current_task_ptr: *mut crate::process::Thread,
}

// Since we only have 1 core, we statically allocate CPU 0's data block.
pub static mut PER_CPU_0: PerCpu = PerCpu {
    cpu_id: 0,
    apic_id: 0,
    kernel_rsp: 0,
    current_task_ptr: core::ptr::null_mut(),
};

pub fn init_cpu_local() {
    let ptr = &raw const PER_CPU_0 as u64;

    let virt_addr = x86_64::VirtAddr::new(ptr);

    x86_64::registers::model_specific::GsBase::write(virt_addr);

    let gs_base = x86_64::registers::model_specific::GsBase::read();
    crate::serial_println!("GS Base: {:#x}", gs_base.as_u64());
}
