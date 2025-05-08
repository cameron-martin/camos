#![no_std]
#![no_main]

//! Memory layout:
//! - 0x0000000000000000: Kernel memory
//! - 0x00000F0000000000: Physical memory

use core::{
    arch::asm,
    fmt::{self, Write},
    mem,
    panic::PanicInfo,
    ptr,
};

use camos_bootinfo::BootInfo;
use elf::{ElfBytes, ParseError, abi::PT_LOAD, endian::AnyEndian};
use thiserror::Error;
use uefi::{
    boot::{
        self, AllocateType, MemoryDescriptor, MemoryType, OpenProtocolAttributes,
        OpenProtocolParams,
    }, mem::memory_map::{MemoryMap, MemoryMapOwned}, prelude::entry, proto::console::gop::{GraphicsOutput, ModeInfo}, runtime::{self, ResetType}, system, Status
};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, MappedPageTable, Mapper, Page, PageSize, PageTable, PageTableFlags,
        PhysFrame, Size4KiB,
        mapper::{MapToError, PageTableFrameMapping},
    },
};

static KERNEL: &[u8] = include_bytes!(env!("KERNEL_PATH"));

const PHYSICAL_MEMORY_START: VirtAddr = VirtAddr::new(0x00000F0000000000);

#[derive(Error, Debug)]
enum LoaderError {
    #[error("cannot parse elf: {0}")]
    ElfParse(ParseError),
    #[error("UEFI error: {0}")]
    UefiError(#[from] uefi::Error),
    #[error("format error")]
    FormatError(#[from] fmt::Error),
    #[error("the kernel elf was invalid")]
    InvalidKernelElf,
    #[error("invalid memory map")]
    InvalidMemoryMap,
    #[error("allocation failed")]
    FailedAllocation,
    #[error("mapping error")]
    MapTo(MapToError<Size4KiB>),
}

impl From<ParseError> for LoaderError {
    fn from(value: ParseError) -> Self {
        Self::ElfParse(value)
    }
}

impl From<MapToError<Size4KiB>> for LoaderError {
    fn from(value: MapToError<Size4KiB>) -> Self {
        Self::MapTo(value)
    }
}

impl From<LoaderError> for uefi::Status {
    fn from(value: LoaderError) -> Self {
        match value {
            LoaderError::UefiError(uefi_error) => uefi_error.status(),
            _ => uefi::Status::ABORTED,
        }
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

    match load_os() {
        Ok(()) => uefi::Status::SUCCESS,
        Err(err) => err.into(),
    }
}

fn load_os() -> Result<(), LoaderError> {
    let elf_file = ElfBytes::<AnyEndian>::minimal_parse(KERNEL)?;

    let root_page_table = create_root_page_table()?;

    system::with_stdout(|out| writeln!(out, "Created root page table!"))?;

    map_kernel(root_page_table, &elf_file)?;

    system::with_stdout(|out| writeln!(out, "Mapped kernel memory!"))?;

    map_physical_memory(root_page_table)?;

    system::with_stdout(|out| writeln!(out, "Mapped physical memory!"))?;

    let (graphics_mode_info, framebuffer) = get_framebuffer()?;

    system::with_stdout(|out| writeln!(out, "Gotten framebuffer!"))?;

    let boot_info = write_boot_info(BootInfo {
        framebuffer,
        graphics_mode_info,
    })?;

    system::with_stdout(|out| writeln!(out, "Written boot info!"))?;

    let memory_map = unsafe { boot::exit_boot_services(None) };

    // system::with_stdout(|out| writeln!(out, "Exited boot services!"))?;

    let entry_addr = elf_file.ehdr.e_entry;

    // system::with_stdout(|out| writeln!(out, "About to jump to {entry_addr}!"))?;

    unsafe {
        asm! {
            "mov rdi, {boot_info}",
            "add rdi, {phys_start}",
            "mov cr3, {page_table}",
            "jmp {entry}",
            boot_info = in(reg) boot_info,
            phys_start = in(reg) PHYSICAL_MEMORY_START.as_u64(),
            page_table = in(reg) root_page_table,
            entry = in(reg) entry_addr,
        }
    }

    unreachable!()
}

fn create_root_page_table() -> Result<&'static mut PageTable, LoaderError> {
    let level_4_table = IdentityFrameMapping.frame_to_pointer(
        UefiFrameAllocator
            .allocate_frame()
            .ok_or(LoaderError::InvalidMemoryMap)?,
    );
    unsafe { ptr::write(level_4_table, PageTable::new()) };
    Ok(unsafe { &mut *level_4_table })
}

/// Create a virtual memory mapping for the kernel executable
fn map_kernel(
    root_page_table: &mut PageTable,
    elf_file: &ElfBytes<AnyEndian>,
) -> Result<(), LoaderError> {
    let mut frame_allocator = UefiFrameAllocator;

    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    let phys_kernel_start = PhysAddr::new(KERNEL.as_ptr() as u64);
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

        let virt_range = Page::<Size4KiB>::range_inclusive(
            Page::containing_address(virt_start),
            Page::containing_address(virt_start + len),
        );

        let phys_range = PhysFrame::<Size4KiB>::range_inclusive(
            PhysFrame::containing_address(phys_start),
            PhysFrame::containing_address(phys_start + len - 1),
        );

        for (page, phys_frame) in virt_range.zip(phys_range) {
            unsafe {
                mapper.map_to(
                    page,
                    phys_frame,
                    PageTableFlags::PRESENT | PageTableFlags::GLOBAL,
                    &mut frame_allocator,
                )
            }?;
        }

        // if virt_end > phys_end {
        //     // TODO: there is a `.bss` section in this segment -> map next
        //     // (virt_end - phys_end) bytes to free physical frame and initialize
        //     // them with zero
        // }
    }

    Ok(())
}

/// Maps the physical memory in the kernel's virtual memory, at an offset
fn map_physical_memory(root_page_table: &mut PageTable) -> Result<(), LoaderError> {
    let memory_map = boot::memory_map(MemoryType::LOADER_DATA)?;

    let max_phys_addr = memory_map
        .entries()
        .map(|desc| desc.phys_start + (desc.page_count * boot::PAGE_SIZE as u64))
        .max()
        .ok_or(LoaderError::InvalidMemoryMap)?;

    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    let virt_range = Page::<Size4KiB>::range_inclusive(
        Page::containing_address(PHYSICAL_MEMORY_START),
        Page::containing_address(PHYSICAL_MEMORY_START + max_phys_addr - 1),
    );

    let phys_range = PhysFrame::<Size4KiB>::range_inclusive(
        PhysFrame::containing_address(PhysAddr::new(0)),
        PhysFrame::containing_address(PhysAddr::new(max_phys_addr) - 1),
    );

    for (page, phys_frame) in virt_range.zip(phys_range) {
        let _ = unsafe {
            mapper.map_to(
                page,
                phys_frame,
                PageTableFlags::PRESENT | PageTableFlags::GLOBAL,
                &mut UefiFrameAllocator,
            )
        }?;
    }

    Ok(())
}

fn get_framebuffer() -> Result<(ModeInfo, *mut u8), LoaderError> {
    let gop_handle = boot::get_handle_for_protocol::<GraphicsOutput>()?;

    let mut protocol = unsafe {
        boot::open_protocol::<GraphicsOutput>(
            OpenProtocolParams {
                handle: gop_handle,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )?
    };

    let mode_info = protocol.current_mode_info();

    let framebuffer = protocol.frame_buffer().as_mut_ptr();

    Ok((mode_info, framebuffer))
}

fn write_boot_info(boot_info: BootInfo) -> Result<*const BootInfo, LoaderError> {
    const _: () = {
        assert!(mem::align_of::<BootInfo>() <= 8);
    };

    let ptr = boot::allocate_pool(MemoryType::LOADER_DATA, mem::size_of::<BootInfo>())?;
    let ptr = ptr.as_ptr() as *mut BootInfo;

    unsafe { ptr::write(ptr, boot_info) };

    Ok(ptr)
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None)
}
