// src/apic.rs

pub struct LocalApic {
    base_addr: x86_64::VirtAddr
}

// Key register offsets
const ID_REG: u64 = 0x20;
const EOI_REG: u64 = 0xB0;
const SPURIOUS_INT_REG: u64 = 0xF0;

impl LocalApic {
    pub unsafe fn new(base_addr: x86_64::VirtAddr) -> Self {
        Self { base_addr } 
    }

    /// Reads a 32-bit register from the APIC
    unsafe fn read_reg(&self, offset: u64) -> u32 {
        let ptr = (self.base_addr.as_u64() + offset) as *const u32;
        core::ptr::read_volatile(ptr)
    }

    /// Writes a 32-bit value to the APIC
    unsafe fn write_reg(&self, offset: u64, value: u32) {
        let ptr = (self.base_addr.as_u64() + offset) as *mut u32;
        core::ptr::write_volatile(ptr, value);
    }

    /// Tells the APIC an interrupt has been handled
    pub fn end_of_interrupt(&self) {
        unsafe {
            // Write 0 to the EOI register
            self.write_reg(EOI_REG, 0);
        }
    }

    pub unsafe fn init(&self) {
        // Enable APIC by setting bit 8 (0x100) and setting spurious vector to 0xFF
        self.write_reg(SPURIOUS_INT_REG, 0x100 | 0xFF);
    }
}

pub static LOCAL_APIC: spin::Mutex<Option<LocalApic>> = spin::Mutex::new(None);
