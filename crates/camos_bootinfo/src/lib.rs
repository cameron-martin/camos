#![no_std]

use uefi::proto::console::gop::ModeInfo;

/// Data passed between the loader and the kernel
#[repr(C)]
pub struct BootInfo {
    /// The address of a port-mapped serial port
    pub serial_base: u16, 
}
