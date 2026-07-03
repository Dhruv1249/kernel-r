// src/interrupts/io_apic.rs

use spin::Mutex;
use x86_64::VirtAddr;

/// Global singleton holding the initialised I/O APIC driver, if any.
///
/// Populated during boot once the I/O APIC's MMIO page has been mapped and
/// the driver constructed.  Protected by a spinlock so it is safe to access
/// from interrupt handlers.
pub static IO_APIC: Mutex<Option<IoApic>> = Mutex::new(None);

/// Driver for the I/O Advanced Programmable Interrupt Controller (I/O APIC).
///
/// The I/O APIC sits between external hardware interrupt sources (keyboard,
/// timer, disk, …) and the CPU's Local APICs.  It receives hardware IRQ lines
/// and routes them to specific interrupt vectors on specific CPU cores according
/// to its **redirection table** (REDTBL).
///
/// Each redirection-table entry is a 64-bit register split into two 32-bit
/// halves accessed via an indirect register interface:
/// - **Index register** at offset `0x00`: write the register number to select.
/// - **Data register** at offset `0x10`: read or write the selected register.
pub struct IoApic {
    base_addr: VirtAddr,
}

impl IoApic {
    /// Constructs an `IoApic` driver pointing at the MMIO window at `base_addr`.
    ///
    /// `base_addr` must be the **virtual** address of the I/O APIC MMIO page
    /// (physical address from the MADT + `PHYS_OFFSET`).
    ///
    /// # Safety
    /// The caller must have mapped the I/O APIC MMIO page into the virtual
    /// address space before calling this function.
    pub unsafe fn new(base_addr: VirtAddr) -> Self {
        Self { base_addr }
    }

    /// Reads a 32-bit register from the I/O APIC at internal index `index`.
    ///
    /// First writes `index` to the index register (offset `0x00`) to select
    /// the target register, then reads the result from the data window (offset
    /// `0x10`).  Both accesses are volatile to prevent compiler reordering.
    ///
    /// # Safety
    /// `index` must be a valid I/O APIC register index and the MMIO page must
    /// be mapped and accessible.
    // Reads a 32-bit register from the I/O APIC
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

    /// Writes `value` to the 32-bit I/O APIC register at internal index `index`.
    ///
    /// Selects the target register by writing `index` to the index register,
    /// then writes `value` to the data register.  Both accesses are volatile.
    ///
    /// # Safety
    /// `index` must be a valid I/O APIC register index and the MMIO page must
    /// be mapped, accessible, and writable.
    // Writes a 32-bit value to the I/O APIC
    pub unsafe fn write_reg(&self, index: u8, value: u32) {
        let index_ptr = self.base_addr.as_u64() as *mut u32;
        let data_ptr = (self.base_addr.as_u64() + 0x10) as *mut u32;

        unsafe {
            core::ptr::write_volatile(index_ptr, index as u32);
            core::ptr::write_volatile(data_ptr, value);
        }
    }

    /// Configures the I/O APIC redirection table to route PS/2 keyboard IRQ 1 to vector 33.
    ///
    /// # Redirection table layout
    ///
    /// Each IRQ has a 64-bit redirection entry at indices `0x10 + irq * 2`
    /// (lower 32 bits) and `0x10 + irq * 2 + 1` (upper 32 bits):
    ///
    /// - **Upper half** (destination): APIC ID of the target CPU core.
    ///   We write 0 to target the BSP (APIC ID 0).
    /// - **Lower half** (vector + flags): interrupt vector number.
    ///   Writing `33` leaves bit 16 (the mask bit) as 0, which **unmasks**
    ///   the IRQ, enabling delivery.  Vector 33 is handled by
    ///   `interrupts::interrupt::keyboard_handler`.
    ///
    /// # Safety
    /// The I/O APIC MMIO page must be mapped and the `IO_APIC` static must
    /// have been initialised before calling this.
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
