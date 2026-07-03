// src/interrupts/apic.rs

/// Driver for the x86_64 Local Advanced Programmable Interrupt Controller (LAPIC).
///
/// The Local APIC is a per-CPU memory-mapped I/O device that manages:
/// - **Interrupt delivery** from the I/O APIC and inter-processor interrupts.
/// - **End-of-interrupt (EOI)** signalling back to the APIC after each ISR.
/// - **The APIC timer**, a high-resolution periodic timer used to drive the
///   kernel scheduler at 1 kHz.
///
/// Registers are accessed as 32-bit volatile reads/writes at fixed offsets from
/// `base_addr`.  The `volatile` qualifier prevents the compiler from caching or
/// reordering MMIO accesses.
pub struct LocalApic {
    base_addr: x86_64::VirtAddr,
}

/// Disables the legacy 8259A Programmable Interrupt Controller (PIC).
///
/// The legacy PIC maps hardware IRQs 0–15 to CPU interrupt vectors 0–15.
/// Vectors 0–7 overlap CPU exception vectors (divide-by-zero, double fault,
/// etc.), causing spurious or misrouted interrupts that instantly crash the
/// kernel.  Masking all IRQs on both the master (port `0x21`) and slave
/// (port `0xA1`) PIC chips by writing `0xFF` silences it completely.
///
/// After this call, the APIC takes over all external interrupt delivery.
///
/// # Safety
/// Writes directly to x86 I/O ports.  Must be called before interrupts are
/// enabled via `sti`.
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

/// Busy-waits for exactly 10 milliseconds using the legacy PIT channel 0.
///
/// Programs PIT channel 0 in **mode 0** (interrupt on terminal count) with a
/// count of 11 931 ticks at the PIT's 1.193182 MHz clock, giving `≈ 10 ms`.
/// Then polls the PIT counter register until it reaches 0.
///
/// This busy-wait is used only once — during APIC timer calibration — to
/// measure how many APIC timer ticks occur in 10 ms.
fn pit_wait_10ms() {
    use x86_64::instructions::port::Port;
    let mut pit_command: Port<u8> = Port::new(0x43);
    let mut pit_data: Port<u8> = Port::new(0x40);

    // 1.193182 MHz * 0.01 seconds = 11931 ticks (0x2E9B)
    let ticks: u16 = 11931;

    unsafe {
        // Command 0x30: Channel 0, Access Lobyte/Hibyte, Mode 0 (Interrupt on terminal count)
        pit_command.write(0x30);
        pit_data.write((ticks & 0xFF) as u8); // Low byte
        pit_data.write(((ticks >> 8) & 0xFF) as u8); // High byte

        // Poll the PIT until it hits 0
        loop {
            // Command 0x00: Latch Channel 0 count
            pit_command.write(0x00);
            let low = pit_data.read();
            let high = pit_data.read();
            let current_count = ((high as u16) << 8) | (low as u16);
            if current_count == 0 {
                break;
            }
        }
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
    /// Constructs a `LocalApic` driver pointing at the MMIO window at `base_addr`.
    ///
    /// `base_addr` must be the **virtual** address of the LAPIC MMIO page,
    /// i.e. the physical APIC address (read from the MADT) plus `PHYS_OFFSET`.
    ///
    /// # Safety
    /// The caller must have already mapped the LAPIC MMIO page into the
    /// virtual address space before calling this function.
    pub unsafe fn new(base_addr: x86_64::VirtAddr) -> Self {
        Self { base_addr }
    }

    /// Reads a 32-bit register at byte `offset` from the LAPIC MMIO base.
    ///
    /// Uses `core::ptr::read_volatile` to prevent the compiler from eliding or
    /// reordering the MMIO read.
    ///
    /// # Safety
    /// `offset` must be a valid LAPIC register offset and the MMIO page must
    /// be mapped and accessible.
    unsafe fn read_reg(&self, offset: u64) -> u32 {
        let ptr = (self.base_addr.as_u64() + offset) as *const u32;
        // Volatile since we dont want the compiler to optimize this out
        unsafe { core::ptr::read_volatile(ptr) }
    }

    /// Writes `value` to the 32-bit register at byte `offset` from the LAPIC MMIO base.
    ///
    /// Uses `core::ptr::write_volatile` to ensure the store reaches the
    /// hardware register and is not optimised away.
    ///
    /// # Safety
    /// `offset` must be a valid LAPIC register offset and the MMIO page must
    /// be mapped, accessible, and writable.
    unsafe fn write_reg(&self, offset: u64, value: u32) {
        let ptr = (self.base_addr.as_u64() + offset) as *mut u32;
        unsafe { core::ptr::write_volatile(ptr, value) };
    }

    /// Signals end-of-interrupt (EOI) to the Local APIC.
    ///
    /// Must be called at the end of every hardware interrupt service routine
    /// (ISR) before returning with `iretq`.  Writing any value to the EOI
    /// register (offset `0xB0`) clears the current interrupt from the APIC's
    /// in-service register (ISR), allowing the APIC to deliver the next
    /// pending interrupt.
    ///
    /// Failing to send EOI will prevent all future APIC interrupts.
    pub fn end_of_interrupt(&self) {
        unsafe {
            // Write 0 to the EOI register
            self.write_reg(EOI_REG, 0);
        }
    }

    /// Enables the Local APIC by writing to the spurious-interrupt vector register.
    ///
    /// Setting bit 8 (`0x100`) of the spurious-interrupt register (`0xF0`)
    /// enables the APIC software-enable flag.  The low byte (`0xFF`) configures
    /// the spurious interrupt vector to 255, which is an unused vector well
    /// above all real interrupt vectors.
    ///
    /// # Safety
    /// Must be called with the MMIO page correctly mapped.
    pub unsafe fn init(&self) {
        // Enable APIC by setting bit 8 (0x100) and setting spurious vector to 0xFF
        unsafe { self.write_reg(SPURIOUS_INT_REG, 0x100 | 0xFF) };
    }

    /// Calibrates the APIC timer against the PIT and starts it in periodic mode at ~1 kHz.
    ///
    /// # Calibration procedure
    ///
    /// 1. Set the divide configuration to `/16`.
    /// 2. Load the initial count register with `0xFFFFFFFF` (maximum).
    /// 3. Wait exactly 10 ms using [`pit_wait_10ms`].
    /// 4. Read the current count register; the difference gives the number of
    ///    APIC timer ticks in 10 ms.
    /// 5. Compute `ticks_per_1ms = ticks_in_10ms / 10`.
    /// 6. Switch the timer to **periodic** mode (bit 17 set in `LVT_TIMER_REG`)
    ///    and reload the initial count with `ticks_per_1ms` so the timer fires
    ///    interrupt vector 32 every ~1 ms (1 kHz).
    ///
    /// The result is that `interrupt::timer_isr` → `process::rust_timer_handler`
    /// is called exactly 1000 times per second, providing 1 ms scheduler ticks.
    pub fn calibrate_and_start_timer(&self) {
        unsafe {
            // Set the interval
            self.write_reg(DIVIDE_CONFIGURATION_REG, 0x3); // Divide by 16
            // Set time mode
            self.write_reg(LVT_TIMER_REG, 32); // one shot mode
            self.write_reg(INIT_COUNT_REG, 0xFFFFFFFF); // Absolute maximum
            pit_wait_10ms();
            let current_count = self.read_reg(CURRENT_COUNT_REG);
            let ticks_in_10ms = 0xFFFFFFFF - current_count;
            let ticks_per_1ms = ticks_in_10ms / 10;
            self.write_reg(LVT_TIMER_REG, (1 << 17) | 32); // Periodic mode
            // Set the initial count to number of ticks in 1ms (1000 Hz)
            self.write_reg(INIT_COUNT_REG, ticks_per_1ms);
            // Now 1 tick = 1,000,000 ns
        }
    }
}

/// Global singleton holding the initialised Local APIC driver, if any.
///
/// Set to `Some(local_apic)` during boot after the LAPIC MMIO page has been
/// mapped and [`LocalApic::init`] has been called.  Protected by a
/// `spin::Mutex` so it can safely be accessed from interrupt handlers.
pub static LOCAL_APIC: spin::Mutex<Option<LocalApic>> = spin::Mutex::new(None);
