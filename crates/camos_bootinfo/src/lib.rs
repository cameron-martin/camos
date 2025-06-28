#![no_std]

use uefi::mem::memory_map::MemoryMapOwned;
use x86_64::{PhysAddr, VirtAddr};

/// Data passed between the loader and the kernel
pub struct BootInfo {
    /// The address of a port-mapped serial port
    pub serial_base: u16,
    pub memory_map: Option<MemoryMapOwned>,
    /// The offset into virtual memory where physical memory is mapped.
    pub physical_offset: VirtAddr,
    /// The number of pages that were allocated for this BootInfo. Useful when
    /// wanting to free the pages associated with the BootInfo.
    pub pages: usize,
}
