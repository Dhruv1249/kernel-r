// src/io_apic.rs

use spin::Mutex;
use x86_64::VirtAddr;

pub static IO_APIC: Mutex<Option<IoApic>> = Mutex::new(None);

pub struct IoApic {
    base_addr: VirtAddr,
}

impl IoApic {
    pub unsafe fn new(base_addr: VirtAddr) -> Self {
        Self { base_addr }
    }

    /// Reads a 32-bit register from the I/O APIC
    pub unsafe fn read_reg(&self, index: u8) -> u32 {
        // The index register is at offset 0x00
        let index_ptr = self.base_addr.as_u64() as *mut u32;
        // The data window is at offset 0x10
        let data_ptr = (self.base_addr.as_u64() + 0x10) as *const u32;

        unsafe {
            core::ptr::write_volatile(index_ptr, index as u32);
            core::ptr::read_volatile(data_ptr)
        }
    }

    /// Writes a 32-bit value to the I/O APIC
    pub unsafe fn write_reg(&self, index: u8, value: u32) {
        let index_ptr = self.base_addr.as_u64() as *mut u32;
        let data_ptr = (self.base_addr.as_u64() + 0x10) as *mut u32;

        unsafe {
            core::ptr::write_volatile(index_ptr, index as u32);
            core::ptr::write_volatile(data_ptr, value);
        }
    }

    pub unsafe fn init_keyboard(&self) {
        // IO APIC Redirection Table Base Index
        const REDTBL_INDEX: u8 = 0x10;

        // IRQ 1 is the keyboard 
        let irq: u8 = 1;
        let lower_index = REDTBL_INDEX + (irq * 2);
        let upper_index = REDTBL_INDEX + (irq * 2) + 1;

       unsafe {  // 1. Write the Upper Half (Destination Core)
        // In a single-core system, APIC ID 0 is the Bootstrap Processor (BSP)
        self.write_reg(upper_index, 0);

        // 2. Write the Lower Half (Vector and Configuration)
        // We want to send this to Vector 33 on the Local APIC.
        // By writing exactly 33, we are leaving the 16th bit (the Mask bit) as 0, 
        // which officially unmasks/enables the interrupt.
        self.write_reg(lower_index, 33); }
    }
}
