//! The kernel process

#![no_std]
#![no_main]

use core::{arch::asm, fmt::Write, panic::PanicInfo};

use camos_bootinfo::BootInfo;
use uart_16550::SerialPort;
use uefi::{
    boot::{self, MemoryType},
    mem::memory_map::{MemoryMap, MemoryMapOwned},
};
use x86_64::structures::paging::{PageSize, Size4KiB};

const PAGE_SIZE: u64 = Size4KiB::SIZE;

#[unsafe(no_mangle)]
extern "C" fn _start(boot_info: &BootInfo) {
    let mut serial = unsafe { SerialPort::new(boot_info.serial_base) };

    writeln!(serial, "Hello from the kernel!");

    find_physical_memory_size(&mut serial, boot_info.memory_map.as_ref().unwrap());

    loop {
        unsafe { asm!("hlt") }
    }
}

fn find_physical_memory_size(serial: &mut SerialPort, memory_map: &MemoryMapOwned) {
    let usable_physical_memory_types = [
        MemoryType::CONVENTIONAL,
        MemoryType::BOOT_SERVICES_CODE,
        MemoryType::BOOT_SERVICES_DATA,
    ];

    let physical_page_count = memory_map
        .entries()
        .filter(|desc| usable_physical_memory_types.contains(&desc.ty))
        .map(|desc| desc.phys_start + (desc.page_count * boot::PAGE_SIZE as u64))
        .max()
        .map(|addr_limit| addr_limit.div_ceil(PAGE_SIZE))
        .unwrap_or(0);

    writeln!(serial, "Physical page count: {}", physical_page_count);
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
