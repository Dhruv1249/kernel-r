use x86_64::instructions::port::Port;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QemuExitCode {
    Success = 0x1,
    Failure = 0x0,
}

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
