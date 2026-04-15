// src/lib.rs

// Adding no_std to the crate allows it to be used in a no-std environment.
// Since rust's standard libraries depends on dependencies like libc, provided by the os.
// We won't be able to use the standard library in this case.
// That's why disabling it here.
#![no_std]
// Also just learned #! -> for whole crate and only # -> for module directly below it!
#![feature(abi_x86_interrupt)] 
// We think rust starts with main, but in reality its entry point is a _start functions which
// sets up the stacks, heap, backtrace for panics etc but all of it is provided in the stdlib
// so we will override the entry point.
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(crate::test_runner)]
pub fn test_runner(tests: &[ ( &str, &dyn Fn()   )]){
    use qemu::QemuExitCode;
    println!("Running {} tests.........",tests.len());
    for test in tests{
        serial_println!("Running {}",test.0);
        test.1();
    }
    exit_qemu(QemuExitCode::Success);
}


// Our imports here.
mod gdt;
mod interrupt;
mod qemu;
mod serial;
mod vga_buffer;
use core::panic::PanicInfo;

use crate::qemu::exit_qemu;





// Defining a panic handler allows us to take care of the error gracefully.
// Again without std, we will have to define a panic handler otherwise it won't compile.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    #[cfg(feature = "test")]
    {
        serial_println!("Test panicked");
        serial_println!("{}", info);
        exit_qemu(crate::qemu::QemuExitCode::Failure);
    }

    println!("{}", info);
    loop {}
}


unsafe extern "C" {
    static stack_bottom: u8;
    static stack_top: u8;
}

// Using no_mangle to disable name mangling.
// Usually whenever rust compiles it gives each functions its own uniquely generated
// cryptic id to differentiate it from all functions (it helps in overloading).
// But in our case _start is the entry point for program and we always want it to have same
// name.
#[unsafe( no_mangle )]
// extern "C" tells rust to call the functions just like C since bootloader expects
// functions to be called specifically like C like register/stack positions and we want
// stability.
pub extern "C" fn _start() -> ! {
    let stack_var = 0u64;
    let stack_addr = &stack_var as *const _ as u64;
    unsafe{
        serial_println!("Stack bottom at {:#x}", &raw const stack_bottom as u64);
        serial_println!("Stack top at {:#x}", &raw const stack_top as u64);
    }
    serial_println!("Stack is at: {:#x}", stack_addr);
    serial_println!("Kernel code at: {:#x}", _start as *const () as u64);
    vga_buffer::clear_screen();
    gdt::init();
    interrupt::load_idt();
    println!("Hello world");
    fn stack_overflow() {
        stack_overflow();
    }
    stack_overflow();
    println!("after stack overflow");
    volatile::Volatile::new(0).read(); // prevent tail recursion optimizations
    loop{}
}


fn testing() {
    assert_eq!(1, 1);
    serial_println!("testing... ok");
}

#[cfg(feature = "test")]
pub fn test_main() {
    serial_println!("Running tests...");
    test_runner(&[( "testing",&testing )]);
}
