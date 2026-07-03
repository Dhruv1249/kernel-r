// src/drivers/qemu.rs

use x86_64::instructions::port::Port;

/// Exit code values for the QEMU ISA debug-exit device (`isa-debug-exit`).
///
/// QEMU applies the formula `(value << 1) | 1` to the written value before
/// using it as the process exit code:
/// - `Success` (`0x1`) → QEMU exits with code `3`  (`(1<<1)|1`)
/// - `Failure` (`0x0`) → QEMU exits with code `1`  (`(0<<1)|1`)
///
/// These codes are used by the kernel's test runner to communicate pass/fail
/// status to the host shell without needing a full VM shutdown sequence.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x1,
    Failure = 0x0,
}

/// Exits QEMU by writing to the ISA debug-exit device at I/O port `0xF4`.
///
/// QEMU maps the `isa-debug-exit` device to port `0xF4`.  Writing a `u32`
/// value to this port causes QEMU to exit immediately.  The actual host
/// process exit code is `(value << 1) | 1` as described in [`QemuExitCode`].
///
/// # Safety
/// Writes directly to an x86 I/O port.  Only meaningful when running inside
/// QEMU with the `isa-debug-exit` device enabled.
pub fn exit_qemu(exit_code: QemuExitCode) {
    // To exit qemu we must write on the port 0xf4
    // The value must be a u32 since it needs 4 bytes
    let mut port: Port<u32> = Port::new(0xf4);

    // Qemu always applies this formula before returning
    // (res << 1) | 1
    // So Success will return ( 0x1 << 1 ) | 1 -> 0x2 | 1 -> 3
    // And Failure will return ( 0x0  << 1 ) | 1 -> 0x0 | 1 -> 1
    unsafe {
        port.write(exit_code as u32);
    }
}
