#![no_std]

use uefi::proto::console::gop::ModeInfo;

/// Data passed between the loader and the kernel
#[repr(C)]
pub struct BootInfo {
    pub framebuffer: *mut u8,
    pub graphics_mode_info: ModeInfo,
}
