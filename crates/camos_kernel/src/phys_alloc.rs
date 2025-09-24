//! Physical memory allocator. Allocates pages of physical memory.

use core::{
    fmt::{self, Write},
    iter,
    ops::Range,
    ptr::{self, NonNull},
    sync::atomic::{self, AtomicU64},
};

use uefi::boot::{self, MemoryDescriptor, MemoryType};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{PageSize, PhysFrame, Size4KiB},
};

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

#[derive(Debug, PartialEq)]
pub struct PageAllocation {
    index: u64,
    /// The offset into virtual memory where the whole of physical memory is mapped
    physical_offset: VirtAddr,
}

impl PageAllocation {
    fn new(index: u64, physical_offset: VirtAddr) -> Self {
        Self {
            index,
            physical_offset,
        }
    }

    pub fn frame(&self) -> PhysFrame<Size4KiB> {
        PhysFrame::from_start_address(PhysAddr::new(self.index * Size4KiB::SIZE)).unwrap()
    }

    pub fn as_ptr(&self) -> NonNull<u8> {
        NonNull::new((self.physical_offset + self.frame().start_address().as_u64()).as_mut_ptr())
            .unwrap()
    }
}

/// An allocator for pages of physical memory. It uses a bitmap to store which
/// pages are allocated, where 1 in the bitmap means free. The bitmap is
/// operated on in 64-bit words, where the least significant bit denotes the
/// availability of the lowest-index page in that range.
pub struct PhysicalMemoryManager {
    bitmap_ptr: *mut u64,
    /// The offset into virtual memory where the whole of physical memory is mapped
    physical_offset: VirtAddr,
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

        let bitmap_size_words = physical_page_count.div_ceil(64);
        // We always address the bitmap in 64-bit words, so we have to
        // allocate enough to accomodate for this
        let bitmap_size_bytes = bitmap_size_words * 8;

        writeln!(
            serial,
            "Physical page count: {}, Bitmap size: {} bytes in {} pages",
            physical_page_count,
            bitmap_size_bytes,
            bitmap_size_bytes.div_ceil(PAGE_SIZE)
        );

        // Find somewhere to put the bitmap pages. This doesn't merge adjacent
        // regions, but it shouldn't matter since we only need to find one place
        // to store this fairly small data structure. The regions will naturally
        // get merged once we add them to the bitmap.
        let bitmap_address = usable_entries(memory_map)
            .find(|&desc| desc.page_count * boot::PAGE_SIZE as u64 >= bitmap_size_bytes)
            .map(|desc| PhysAddr::new(desc.phys_start))
            .expect("Cannot find bitmap address");

        // The page indices that are taken by the bitmap itself
        let bitmap_page_range = (bitmap_address.as_u64() / PAGE_SIZE)
            ..(bitmap_address.as_u64() + bitmap_size_bytes).div_ceil(PAGE_SIZE);

        // The location of the bitmap in virtual memory
        let bitmap_ptr = (physical_offset.as_u64() + bitmap_address.as_u64()) as *mut u64;

        writeln!(
            serial,
            "Bitmap at {:#X} in physical memory and {:#X} in virtual memory",
            bitmap_address, bitmap_ptr as usize
        );

        // Zero out the bitmap. Zero here means not available, and available
        // regions will be explicitly added.
        unsafe {
            ptr::write_bytes(bitmap_ptr, 0, bitmap_size_words as usize);
        }

        let manager = Self {
            bitmap_ptr,
            page_count: physical_page_count,
            physical_offset,
        };

        for desc in usable_entries(memory_map) {
            // UEFI pages are always 4KiB, regardless of the kernel's page size
            let phys_end = desc.phys_start + desc.page_count * boot::PAGE_SIZE as u64;

            // The range of indices of kernel-sized page in this region.
            let page_start = desc.phys_start.div_ceil(PAGE_SIZE);
            let page_end = phys_end / PAGE_SIZE;

            for page_index in page_start..page_end {
                if !bitmap_page_range.contains(&page_index) {
                    manager.free_page(page_index);
                }
            }
        }

        manager
    }

    /// Returns the byte containing the bit for a certain page, plus a mask for the bit
    fn get_word(&self, page_index: u64) -> (&AtomicU64, u64) {
        let word_offset = page_index / 64;
        let bit_offset = page_index % 64;

        let mask = 1u64 << bit_offset;

        let byte = unsafe { AtomicU64::from_ptr(self.bitmap_ptr.add(word_offset as usize)) };

        (byte, mask)
    }

    pub fn free_page(&self, page_index: u64) {
        let (word, mask) = self.get_word(page_index);

        word.fetch_or(mask, atomic::Ordering::Relaxed);
    }

    /// Allocates a single physical page. Returns the index to it
    pub fn allocate_page(&self) -> Option<PageAllocation> {
        let mut ptr = self.bitmap_ptr as *mut u64;
        // The page index that the current bitmap word starts at
        let mut idx_start = 0;

        while idx_start < self.page_count {
            let bitmap_word = unsafe { AtomicU64::from_ptr(ptr) };

            let mut prev = bitmap_word.load(atomic::Ordering::SeqCst);
            loop {
                // No pages are available in this word
                if prev == 0 {
                    break;
                }

                // Zero out the first set bit
                let next = prev & !(1u64 << prev.trailing_zeros());
                match bitmap_word.compare_exchange_weak(
                    prev,
                    next,
                    atomic::Ordering::SeqCst,
                    atomic::Ordering::SeqCst,
                ) {
                    Ok(_) => {
                        let page_index = idx_start + prev.trailing_zeros() as u64;

                        return Some(PageAllocation::new(page_index, self.physical_offset));
                    }
                    Err(next_prev) => prev = next_prev,
                }
            }

            ptr = unsafe { ptr.add(1) };
            idx_start += 64;
        }

        None
    }

    fn is_available(&self, page_index: u64) -> bool {
        let (word, mask) = self.get_word(page_index);

        word.load(atomic::Ordering::Relaxed) & mask != 0
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
        alloc, format,
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

    /// Tests that we are computing the usable ranges correctly, with one
    /// two-page usable range. The first will be the memory map and the second
    /// will be available for allocations.
    #[test]
    fn avilable_regions_with_single_usable_range() {
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

    /// Tests that we are computing the usable ranges correctly, with 6 pages.
    /// The first page will be used for the memory map and the rest will be
    /// usable, except for one page in the middle.
    #[test]
    fn avilable_regions_with_multiple_usable_ranges() {
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

    /// Tests that we can allocate pages.
    #[test]
    fn page_allocation() {
        let mut serial = String::new();

        let memory = PhysicalMemory::new(10);

        let memory_map = TestMemoryMap::new(vec![MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            att: MemoryAttribute::empty(),
            page_count: 10,
            phys_start: 0,
            virt_start: 0,
        }]);

        let memory_manager = PhysicalMemoryManager::init(
            &mut serial,
            &memory_map,
            VirtAddr::from_ptr(memory.allocation),
        );

        let expected_page_index = 1;

        assert!(memory_manager.is_available(expected_page_index));

        let actual_allocation = memory_manager.allocate_page().unwrap();

        assert_eq!(actual_allocation.index, expected_page_index);

        assert!(!memory_manager.is_available(expected_page_index));
    }

    /// Not all bits in the last word in the bitmap will be valid, if the
    /// number of pages does not happen to be a multiple of 64. This tests that
    /// we limit the last word, avoiding allocation of physical memory that
    /// does not exist.
    #[test]
    fn page_allocation_overflow() {
        let mut serial = String::new();

        let memory = PhysicalMemory::new(7);

        let memory_map = TestMemoryMap::new(vec![MemoryDescriptor {
            ty: MemoryType::CONVENTIONAL,
            att: MemoryAttribute::empty(),
            page_count: 7,
            phys_start: 0,
            virt_start: 0,
        }]);

        let memory_manager = PhysicalMemoryManager::init(
            &mut serial,
            &memory_map,
            VirtAddr::from_ptr(memory.allocation),
        );

        // One page is taken up by the bitmap, so we can only allocate 6 pages
        for _ in 0..6 {
            memory_manager.allocate_page().unwrap();
        }

        assert_eq!(memory_manager.allocate_page(), None);
    }
}
