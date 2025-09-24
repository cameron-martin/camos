//! The kernel process

#![no_std]
#![no_main]

extern crate alloc;

use core::{
    arch::asm,
    cell::UnsafeCell,
    fmt::Write,
    mem::{self, MaybeUninit},
    panic::PanicInfo,
    ptr,
};

use alloc::string::String;

use camos_bootinfo::BootInfo;
use camos_kernel_lib::{dynamic_alloc::DynamicAllocator, phys_alloc::PhysicalMemoryManager};
use uart_16550::SerialPort;

struct PmmWrapper(UnsafeCell<MaybeUninit<PhysicalMemoryManager>>);

static PMM: PmmWrapper = PmmWrapper(UnsafeCell::new(MaybeUninit::uninit()));

unsafe impl Sync for PmmWrapper {}

// SAFETY: PMM initialised at the very beginning of _start
#[global_allocator]
static mut ALLOCATOR: DynamicAllocator =
    unsafe { DynamicAllocator::new(PMM.0.get() as *const PhysicalMemoryManager) };

#[unsafe(no_mangle)]
extern "C" fn _start(boot_info: &BootInfo) {
    let mut serial = unsafe { SerialPort::new(boot_info.serial_base) };

    unsafe {
        ptr::write(
            PMM.0.get(),
            MaybeUninit::new(PhysicalMemoryManager::init(
                &mut serial,
                boot_info.memory_map.as_ref().unwrap(),
                boot_info.physical_range.start,
            )),
        )
    };

    writeln!(serial, "Hello from the kernel!");

    let pmm = unsafe { &*(PMM.0.get() as *const PhysicalMemoryManager) };

    writeln!(serial, "{}", pmm);

    let foo = String::from("This is a dynamically-allocated string!");

    writeln!(serial, "{}", foo);

    mem::drop(foo);

    loop {
        unsafe { asm!("hlt") }
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
