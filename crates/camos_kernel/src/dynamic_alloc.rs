use core::{
    alloc::{GlobalAlloc, Layout},
    array, cmp,
    ptr::{self, NonNull},
};

use crate::{
    PAGE_SIZE,
    phys_alloc::PhysicalMemoryManager,
    spinlock::{SpinLock, SpinLockGuard},
};

/// The minimum power-of-two that the allocator supports allocating. Any
/// allocations smaller than this will need to be rounded up.
const MIN_POWER: u32 = 3;

/// A dynamic memory allocator. Currently it's a really naive implementation
/// that can only acquire pages of physical memory but not release them. It
/// rounds a requested allocation up to the nearest power of two, then
/// allocates it from a dedicated freelist for that size. When the freelist is
/// empty, a new physical page is carved up and added to the freelist. The
/// allocations point directly into the kernel's linear mapping of physical
/// memory. Partially due to this, the maximum size of an allocation is one
/// page.
pub struct DynamicAllocator {
    pmm: *const PhysicalMemoryManager,
    /// Freelists for 8-4096 byte general purpose allocations
    freelists: [SpinLock<Freelist>; 10],
}

impl DynamicAllocator {
    /// Constructs the allocator
    ///
    /// # Safety
    ///
    /// By the time the allocator is used, the passed-in PMM must point to
    /// initialised memory.
    pub const unsafe fn new(pmm: *const PhysicalMemoryManager) -> Self {
        const fn init_i(i: u32) -> SpinLock<Freelist> {
            SpinLock::new(Freelist::new(1 << (i + MIN_POWER)))
        }

        Self {
            pmm,
            freelists: [
                init_i(0),
                init_i(1),
                init_i(2),
                init_i(3),
                init_i(4),
                init_i(5),
                init_i(6),
                init_i(7),
                init_i(8),
                init_i(9),
            ],
        }
    }

    fn freelist_index(layout: Layout) -> usize {
        // If alignment is larger than the requested size, we have to assign a
        // larger block size to get a larger alignment
        let size_align_max = cmp::max(layout.size(), layout.align());

        // TODO: Think more about what to do if size and/or alignment is zero
        if size_align_max == 0 {
            panic!("Invalid size or alignment")
        }

        let ceil_log2 = usize::BITS - (size_align_max - 1).leading_zeros();

        // The freelists start at 8 bytes (2^3), so account for this.
        ceil_log2.checked_sub(MIN_POWER).unwrap_or(0) as usize
    }

    fn freelist(&self, layout: Layout) -> Option<SpinLockGuard<Freelist>> {
        let index = Self::freelist_index(layout);

        let freelist = self.freelists.get(index)?.lock();

        debug_assert!(layout.size() <= freelist.block_size);
        debug_assert!(layout.align() <= freelist.block_size);
        debug_assert!(layout.size() > freelist.block_size / 2);
        debug_assert!(layout.align() > freelist.block_size / 2);

        Some(freelist)
    }

    pub fn allocate(&self, layout: Layout) -> Option<NonNull<u8>> {
        let mut freelist = self.freelist(layout)?;

        freelist.pop_block().or_else(|| {
            freelist.allocate_new_page(unsafe { &*self.pmm }).then(|| {
                // Allocating the page above will ensure there will always be a
                // block to pop.
                freelist.pop_block().unwrap()
            })
        })
    }

    pub fn deallocate(&self, ptr: NonNull<u8>, layout: Layout) {
        // TODO: Can we do something better here than panic?
        let mut freelist = self.freelist(layout).unwrap();

        freelist.push_block(ptr);
    }
}

unsafe impl GlobalAlloc for DynamicAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.allocate(layout)
            .map(|ptr| ptr.as_ptr())
            .unwrap_or(ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if let Some(ptr) = NonNull::new(ptr) {
            self.deallocate(ptr, layout);
        }
    }
}

#[repr(transparent)]
#[derive(Copy, Clone)]
struct FreelistPointer(Option<NonNull<FreelistPointer>>);

struct Freelist {
    block_size: usize,
    head: FreelistPointer,
}

impl Freelist {
    const fn new(block_size: usize) -> Self {
        Self {
            block_size,
            head: FreelistPointer(None),
        }
    }

    fn push_block(&mut self, block: NonNull<u8>) {
        let block = block.cast::<FreelistPointer>();

        unsafe {
            ptr::write(block.as_ptr(), self.head);
        }

        self.head = FreelistPointer(Some(block));
    }

    fn pop_block(&mut self) -> Option<NonNull<u8>> {
        let ptr = self.head.0?;

        self.head = unsafe { *ptr.as_ptr() };

        Some(ptr.cast())
    }

    /// Allocates a new physical page, splits it up into blocks and adds them
    /// to the freelist. Returns whether this process was successful.
    fn allocate_new_page(&mut self, pmm: &PhysicalMemoryManager) -> bool {
        let Some(page_allocation) = pmm.allocate_page() else {
            return false;
        };

        let block_size = self.block_size;

        let mut addr = page_allocation.as_ptr();

        let addr_end = unsafe { addr.add(PAGE_SIZE as usize) };

        while addr < addr_end {
            self.push_block(addr);

            addr = unsafe { addr.add(block_size) };
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_freelist_index() {
        assert_eq!(
            DynamicAllocator::freelist_index(Layout::from_size_align(2, 1).unwrap()),
            0
        );

        assert_eq!(
            DynamicAllocator::freelist_index(Layout::from_size_align(6, 4).unwrap()),
            0
        );

        assert_eq!(
            DynamicAllocator::freelist_index(Layout::from_size_align(8, 4).unwrap()),
            0
        );

        assert_eq!(
            DynamicAllocator::freelist_index(Layout::from_size_align(12, 4).unwrap()),
            1
        );
    }
}
