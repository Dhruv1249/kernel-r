// main.rs

// Adding no_std to the crate allows it to be used in a no-std environment.
// Since rust's standard libraries depends on dependencies like libc, provided by the os.
// We won't be able to use the standard library in this case.
// That's why disabling it here.
#![no_std]
// Also just learned #! -> for whole crate and only # -> for module directly below it!

// We think rust starts with main, but in reality its entry point is a _start functions which
// sets up the stacks, heap, backtrace for panics etc but all of it is provided in the stdlib
// so we will override the entry point.
#![no_main]

// Importing vga buffer library to print on screen.
mod vga_buffer;
use core::panic::PanicInfo;

// Defining a panic handler allows us to take care of the error gracefully.
// Again without std, we will have to define a panic handler otherwise it won't compile.
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}


// Using no_mangle to disable name mangling.
// Usually whenever rust compiles it gives each functions its own uniquely generated
// cryptic id to differentiate it from all functions (it helps in overloading).
// But in our case _start is the entry point for program and we always want it to have same
// name.





fn clear_screen(){
    let vga_buffer = 0xb8000 as *mut u8;
    for i in 0..80*25{
        unsafe{
            *vga_buffer.offset(i as isize *2) = 0x20;
            *vga_buffer.offset(i as isize *2 +1) = 0x07;
        }
    }
}

#[unsafe( no_mangle )]
// extern "C" tells rust to call the functions just like C since bootloader expects
// functions to be called specifically like C like register/stack positions and we want
// stability.
pub extern "C" fn _start() -> !{
    
    clear_screen();
    vga_buffer::print_something();
    loop{}
}
