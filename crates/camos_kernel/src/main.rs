//! The kernel process

#![no_std]
#![no_main]

use core::{arch::asm, fmt::Write, panic::PanicInfo};

use camos_bootinfo::BootInfo;
use uart_16550::SerialPort;

#[unsafe(no_mangle)]
extern "C" fn _start(boot_info: &BootInfo) {
    let mut serial = unsafe { SerialPort::new(boot_info.serial_base) };

    writeln!(serial, "Hello from the kernel!");

    loop {
        unsafe { asm!("hlt") }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
