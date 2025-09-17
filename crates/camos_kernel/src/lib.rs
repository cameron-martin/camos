#![no_std]

use x86_64::structures::paging::{PageSize, Size4KiB};

mod memory_map;
pub mod phys_alloc;
pub mod spinlock;

/// The kernel's page size. This could be different from the UEFI page size
pub const PAGE_SIZE: u64 = Size4KiB::SIZE;
