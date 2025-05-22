#![no_std]

use uefi::{mem::memory_map::MemoryMapOwned, proto::console::gop::ModeInfo};

/// Data passed between the loader and the kernel
#[repr(C)]
pub struct BootInfo {
    /// The address of a port-mapped serial port
    pub serial_base: u16,
    pub memory_map: Option<MemoryMapOwned>,
    /// The number of pages that were allocated for this BootInfo. Useful when
    /// wanting to free the pages associated with the BootInfo.
    pub pages: usize,
}
