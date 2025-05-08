//! The kernel process

#![no_std]
#![no_main]

use core::panic::PanicInfo;

use camos_bootinfo::BootInfo;

#[unsafe(no_mangle)]
extern "C" fn _start(boot_info: &BootInfo) {
    loop {}
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
