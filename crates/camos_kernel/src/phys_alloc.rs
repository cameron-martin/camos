//! Physical memory allocator. Allocates pages of physical memory.

use core::{
    fmt::{self, Write},
    iter,
    ops::Range,
    ptr,
    sync::atomic::{self, AtomicU8},
};

use uart_16550::SerialPort;
use uefi::boot::{self, MemoryDescriptor, MemoryType};
use x86_64::{PhysAddr, VirtAddr};

use crate::{PAGE_SIZE, memory_map::MemoryMap};

const USABLE_PHYSICAL_MEMORY_TYPES: &'static [MemoryType] = &[
    MemoryType::CONVENTIONAL,
    MemoryType::BOOT_SERVICES_CODE,
    MemoryType::BOOT_SERVICES_DATA,
];

fn usable_entries<'a>(
    memory_map: &'a impl MemoryMap,
) -> impl Iterator<Item = &'a MemoryDescriptor> {
    memory_map
        .entries()
        .filter(|desc| USABLE_PHYSICAL_MEMORY_TYPES.contains(&desc.ty))
}

pub struct PhysicalMemoryManager {
    bitmap_ptr: *mut u8,
    /// The number of physical pages managed by this bitmap. Note this is _not_
    /// the number of pages the bitmap takes up in memory.
    page_count: u64,
}

impl PhysicalMemoryManager {
    pub fn init(
        serial: &mut impl Write,
        memory_map: &impl MemoryMap,
        physical_offset: VirtAddr,
    ) -> Self {
        let physical_page_count = usable_entries(memory_map)
            .map(|desc| desc.phys_start + (desc.page_count * boot::PAGE_SIZE as u64))
            .max()
            .map(|addr_limit| addr_limit.div_ceil(PAGE_SIZE))
            .unwrap_or(0);

        // Bitmap size in bytes
        let bitmap_size = physical_page_count.div_ceil(8);

        writeln!(
            serial,
            "Physical page count: {}, Bitmap size: {} bytes in {} pages",
            physical_page_count,
            bitmap_size,
            bitmap_size.div_ceil(PAGE_SIZE)
        );

        // Find somewhere to put the bitmap pages. This doesn't merge adjacent
        // regions, but it shouldn't matter since we only need to find one place
        // to store this fairly small data structure. The regions will naturally
        // get merged once we add them to the bitmap.
        let bitmap_address = usable_entries(memory_map)
            .find(|&desc| desc.page_count * boot::PAGE_SIZE as u64 >= bitmap_size)
            .map(|desc| PhysAddr::new(desc.phys_start))
            .expect("Cannot find bitmap address");

        // This page indices that are taken by the bitmap itself
        let bitmap_page_range = (bitmap_address.as_u64() / PAGE_SIZE)
            ..(bitmap_address.as_u64() + bitmap_size).div_ceil(PAGE_SIZE);

        // The location of the bitmap in virtual memory
        let bitmap_ptr = (physical_offset.as_u64() + bitmap_address.as_u64()) as *mut u8;

        writeln!(
            serial,
            "Bitmap at {:#X} in physical memory and {:#X} in virtual memory",
            bitmap_address, bitmap_ptr as usize
        );

        // Zero out the bitmap. Zero here means not available, and available
        // regions will be explicitly added.
        unsafe {
            ptr::write_bytes(bitmap_ptr, 0, bitmap_size as usize);
        }

        let manager = Self {
            bitmap_ptr,
            page_count: physical_page_count,
        };

        for desc in usable_entries(memory_map) {
            // UEFI pages are always 4KiB, regardless of the kernel's page size
            let phys_end = desc.phys_start + desc.page_count * boot::PAGE_SIZE as u64;

            // The range of indices of kernel-sized page in this region.
            let page_start = desc.phys_start.div_ceil(PAGE_SIZE);
            let page_end = phys_end / PAGE_SIZE;

            for page_index in page_start..page_end {
                if !bitmap_page_range.contains(&page_index) {
                    manager.free_page_by_index(page_index);
                }
            }
        }

        manager
    }

    /// Returns the byte, plus a mask for the bit
    fn get_bit(&self, page_index: u64) -> (&AtomicU8, u8) {
        let byte_offset = page_index / 8;
        let bit_offset = page_index % 8;

        let mask = 1 << (7 - bit_offset);

        let byte = unsafe { AtomicU8::from_ptr(self.bitmap_ptr.add(byte_offset as usize)) };

        (byte, mask)
    }

    fn free_page_by_index(&self, page_index: u64) {
        let (byte, mask) = self.get_bit(page_index);

        byte.fetch_or(mask, atomic::Ordering::Relaxed);
    }

    fn is_available(&self, page_index: u64) -> bool {
        let (byte, mask) = self.get_bit(page_index);

        byte.load(atomic::Ordering::Relaxed) & mask != 0
    }

    fn available_regions(&self) -> impl Iterator<Item = Range<u64>> {
        let mut i = 0;

        iter::from_fn(move || {
            while !self.is_available(i) && i < self.page_count {
                i += 1;
            }

            // i has gone out of bounds of the physical memory
            if i >= self.page_count {
                return None;
            }

            let start = i;

            while self.is_available(i) && i < self.page_count {
                i += 1;
            }

            Some(start..i)
        })
    }
}

impl fmt::Display for PhysicalMemoryManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Available Memory Regions:")?;
        for region in self.available_regions() {
            writeln!(
                f,
                "{:#X}..{:#X}: {} pages",
                region.start * PAGE_SIZE,
                region.end * PAGE_SIZE,
                region.end - region.start,
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    use core::alloc::Layout;
    use std::{
        alloc, format, io, println,
        string::{String, ToString},
        vec,
    };

    use uefi::boot::{MemoryAttribute, MemoryDescriptor};

    use crate::{PAGE_SIZE, memory_map::TestMemoryMap};

    /// An allocation that acts as the whole region of physical memory for the
    /// purpose of tests
    struct PhysicalMemory {
        allocation: *mut u8,
        num_pages: u64,
        layout: Layout,
    }

    impl PhysicalMemory {
        fn new(num_pages: u64) -> Self {
            let layout =
                Layout::from_size_align((PAGE_SIZE * num_pages) as usize, PAGE_SIZE as usize)
                    .unwrap();
            let allocation = unsafe { alloc::alloc(layout) };
            if allocation.is_null() {
                alloc::handle_alloc_error(layout);
            }

            Self {
                allocation,
                num_pages,
                layout,
            }
        }
    }

    impl Drop for PhysicalMemory {
        fn drop(&mut self) {
            unsafe { alloc::dealloc(self.allocation, self.layout) };
        }
    }

    /// One two-page usable range. The first will be the memory map and the
    /// second will be available for allocations
    #[test]
    fn single_usable_range() {
        let mut serial = String::new();

        let memory = PhysicalMemory::new(2);

        let memory_map = TestMemoryMap::new(vec![MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            att: MemoryAttribute::empty(),
            page_count: 2,
            phys_start: 0,
            virt_start: 0,
        }]);

        let memory_manager = PhysicalMemoryManager::init(
            &mut serial,
            &memory_map,
            VirtAddr::from_ptr(memory.allocation),
        );

        assert!(!memory_manager.is_available(0));
        assert!(memory_manager.is_available(1));

        assert_eq!(
            format!("{}", memory_manager),
            "Available Memory Regions:\n0x1000..0x2000: 1 pages\n".to_string()
        );
    }

    /// 6 pages. The first page will be used for the memory map and the rest
    /// will be usable, except for one page in the middle:
    #[test]
    fn multiple_usable_range() {
        let mut serial = String::new();

        let memory = PhysicalMemory::new(6);

        let memory_map = TestMemoryMap::new(vec![
            MemoryDescriptor {
                ty: MemoryType::CONVENTIONAL,
                att: MemoryAttribute::empty(),
                page_count: 3,
                phys_start: 0,
                virt_start: 0,
            },
            MemoryDescriptor {
                ty: MemoryType::CONVENTIONAL,
                att: MemoryAttribute::empty(),
                page_count: 2,
                phys_start: PAGE_SIZE * 4,
                virt_start: 0,
            },
        ]);

        let memory_manager = PhysicalMemoryManager::init(
            &mut serial,
            &memory_map,
            VirtAddr::from_ptr(memory.allocation),
        );

        // println!("{}", serial);

        assert!(!memory_manager.is_available(0));
        assert!(memory_manager.is_available(1));
        assert!(memory_manager.is_available(2));
        assert!(!memory_manager.is_available(3));
        assert!(memory_manager.is_available(4));
        assert!(memory_manager.is_available(5));

        assert_eq!(
            format!("{}", memory_manager),
            "Available Memory Regions:\n0x1000..0x3000: 2 pages\n0x4000..0x6000: 2 pages\n"
                .to_string()
        );
    }
}
