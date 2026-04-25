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
}
