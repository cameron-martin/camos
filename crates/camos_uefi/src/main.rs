#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo, ptr};

use elf::{ElfBytes, ParseError, abi::PT_LOAD, endian::AnyEndian};
use thiserror::Error;
use uefi::{
    boot::{self, AllocateType, MemoryDescriptor, MemoryType},
    mem::memory_map::{MemoryMap, MemoryMapOwned},
    prelude::entry,
    system,
};
use x86_64::{
    structures::paging::{
        mapper::PageTableFrameMapping, FrameAllocator, MappedPageTable, Mapper, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB
    }, PhysAddr, VirtAddr
};

static KERNEL: &[u8] = include_bytes!(env!("KERNEL_PATH"));

#[derive(Error, Debug)]
enum LoaderError {
    #[error("cannot parse elf: {0}")]
    ElfParse(ParseError),
    #[error("the kernel elf was invalid")]
    InvalidKernelElf,
    #[error("invalid memory map")]
    InvalidMemoryMap,
    #[error("allocation failed")]
    FailedAllocation,
}

impl From<ParseError> for LoaderError {
    fn from(value: ParseError) -> Self {
        Self::ElfParse(value)
    }
}

struct UefiFrameAllocator;

unsafe impl FrameAllocator<Size4KiB> for UefiFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let frame_addr =
            boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1).ok()?;

        PhysFrame::from_start_address(PhysAddr::new(frame_addr.addr().get() as u64)).ok()
    }
}

struct IdentityFrameMapping;

unsafe impl PageTableFrameMapping for IdentityFrameMapping {
    fn frame_to_pointer(&self, frame: PhysFrame) -> *mut PageTable {
        VirtAddr::new(frame.start_address().as_u64()).as_mut_ptr()
    }
}

#[entry]
fn efi_main() -> uefi::Status {
    let Ok(_) = system::with_stdout(|out| writeln!(out, "Hello from Rust!")) else {
        return uefi::Status::WARN_WRITE_FAILURE;
    };

    let memory_map = unsafe { boot::exit_boot_services(None) };

    map_kernel();

    // let entry_addr = elf_file.ehdr.e_entry;

    // Jump to entry_addr

    uefi::Status::SUCCESS
}

/// Create a virtual memory mapping for the kernel executable
fn map_kernel() -> Result<(), LoaderError> {
    let mut frame_allocator = UefiFrameAllocator;

    let level_4_table = IdentityFrameMapping.frame_to_pointer(
        frame_allocator
            .allocate_frame()
            .ok_or(LoaderError::InvalidMemoryMap)?,
    );
    unsafe { ptr::write(level_4_table, PageTable::new()) };
    let level_4_table = &mut unsafe { *level_4_table };

    let mut mapper = unsafe { MappedPageTable::new(level_4_table, IdentityFrameMapping) };

    let phys_kernel_start = PhysAddr::new(KERNEL.as_ptr() as u64);
    let elf_file = ElfBytes::<AnyEndian>::minimal_parse(KERNEL)?;
    let segments = elf_file.segments().ok_or(LoaderError::InvalidKernelElf)?;

    let load_segments = segments
        .into_iter()
        .filter(|segment| segment.p_type == PT_LOAD);

    for segment in load_segments {
        let phys_start = phys_kernel_start + segment.p_offset;
        let len = segment.p_filesz;

        let virt_start = VirtAddr::new(segment.p_vaddr);
        let virt_end = virt_start + segment.p_memsz;

        // let permissions = permissions_from_flags(segment.p_flags);

        for frame in PhysFrame::<Size4KiB>::range_inclusive(
            PhysFrame::from_start_address(phys_start).map_err(|_| LoaderError::InvalidKernelElf)?,
            PhysFrame::containing_address(phys_start + len - 1),
        ) {
            unsafe {
                mapper.map_to(
                    Page::from_start_address(virt_start)
                        .map_err(|_| LoaderError::InvalidKernelElf)?,
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::GLOBAL,
                    &mut frame_allocator,
                )
            };
        }

        // if virt_end > phys_end {
        //     // TODO: there is a `.bss` section in this segment -> map next
        //     // (virt_end - phys_end) bytes to free physical frame and initialize
        //     // them with zero
        // }
    }

    Ok(())
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
