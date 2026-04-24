// src/apic.rs

pub struct LocalApic {
    base_addr: x86_64::VirtAddr,
}

pub unsafe fn disable_legacy_pic() {
    use x86_64::instructions::port::Port;

    // The legacy PIC uses ports 0x21 (Master) and 0xA1 (Slave) for data
    let mut pic1_data: Port<u8> = Port::new(0x21);
    let mut pic2_data: Port<u8> = Port::new(0xA1);

    // Writing 0xFF (11111111) to the data ports masks (disables) all interrupts
    unsafe {
        pic1_data.write(0xFF);
        pic2_data.write(0xFF);
    }
}

// Key register offsets
// Id register used to read the current APIC ID and identify the CPU
const ID_REG: u64 = 0x20;
// End of interrupt register signalling to the CPU that an interrupt has been handled
const EOI_REG: u64 = 0xB0;
// Spurious interrupt register used to tell the APIC that we want to send a spurious interrupt
// Spurious interrupts happens when the an interrupt is fired but there is nothing to service
const SPURIOUS_INT_REG: u64 = 0xF0;
// Controls the timer's mode (One-shot vs. Periodic) and tells the APIC which interrupt vector to fire
const LVT_TIMER_REG: u64 = 0x320;
const INIT_COUNT_REG: u64 = 0x380;
const CURRENT_COUNT_REG: u64 = 0x390;
// Controls the timert ticks's speed relative to the core clock
// Bit pattern 0000 (0x0) = Divide by 2
// Bit pattern 0001 (0x1) = Divide by 4
// Bit pattern 0010 (0x2) = Divide by 8
// Bit pattern 0011 (0x3) = Divide by 16
const DIVIDE_CONFIGURATION_REG: u64 = 0x3E0;

impl LocalApic {
    pub unsafe fn new(base_addr: x86_64::VirtAddr) -> Self {
        Self { base_addr }
    }

    /// Reads a 32-bit register from the APIC
    unsafe fn read_reg(&self, offset: u64) -> u32 {
        let ptr = (self.base_addr.as_u64() + offset) as *const u32;
        // Volatile since we dont want the compiler to optimize this out
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Writes a 32-bit value to the APIC
    unsafe fn write_reg(&self, offset: u64, value: u32) {
        let ptr = (self.base_addr.as_u64() + offset) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value) };
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
        unsafe { self.write_reg(SPURIOUS_INT_REG, 0x100 | 0xFF) };
    }

    pub fn start_timer(&self) {
        unsafe {
            // Set the timer mode
            self.write_reg(DIVIDE_CONFIGURATION_REG, 0x3);
            // Set the timer interval
            self.write_reg(LVT_TIMER_REG, (1 << 17) | 32);
            self.write_reg(INIT_COUNT_REG, 0x10000);
        }
    }
}

pub static LOCAL_APIC: spin::Mutex<Option<LocalApic>> = spin::Mutex::new(None);
