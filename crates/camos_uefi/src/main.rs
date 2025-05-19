#![no_std]
#![no_main]

mod acpi;
mod output;

use core::{arch::asm, fmt, mem, panic::PanicInfo, ptr};

use camos_bootinfo::BootInfo;
use elf::{ElfBytes, ParseError, abi::PT_LOAD, endian::AnyEndian};
use output::COM1;
use thiserror::Error;
use uefi::{
    boot::{self, AllocateType, MemoryType},
    mem::memory_map::MemoryMap,
    prelude::entry,
};
use x86_64::{
    PhysAddr, VirtAddr,
    structures::paging::{
        FrameAllocator, MappedPageTable, Mapper, Page, PageSize, PageTable, PageTableFlags,
        PhysFrame, Size4KiB,
        mapper::{MapToError, PageTableFrameMapping},
        page::AddressNotAligned,
    },
};

type Result<T> = core::result::Result<T, LoaderError>;

static KERNEL: &[u8] = include_bytes!(env!("KERNEL_PATH"));
/// A tiny piece of code that is mapped the virtual address space of both the
/// kernel and the bootloader, to facilitate transfer of control between the
/// bootloader and the kernel.
static TRAMPOLINE: &[u8] = include_bytes!(env!("TRAMPOLINE_PATH"));

/// The address where physical memory is mapped.
const PHYSICAL_MEMORY_START: VirtAddr = VirtAddr::new(0xFFFF880000000000);
const PHYSICAL_MEMORY_END: VirtAddr = STACK_START;

/// The address of the start of the kernel executable
const KERNEL_START: VirtAddr = VirtAddr::new(0xFFFFFFFF80000000);
const KERNEL_END: VirtAddr = STACK_START;

/// The address of the start of the region of virtual memory containing kernel stacks.
const STACK_START: VirtAddr = VirtAddr::new(0xFFFFFFFF90000000);

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
    #[error("mapping error: {0:?}")]
    MapTo(MapToError<Size4KiB>),
    #[error("page address not aligned to a page boundary")]
    AddressNotAligned,
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

impl From<AddressNotAligned> for LoaderError {
    fn from(_: AddressNotAligned) -> Self {
        Self::AddressNotAligned
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
    match load_os() {
        Ok(()) => uefi::Status::SUCCESS,
        Err(err) => panic!("{err}"),
    }
}

fn load_os() -> Result<()> {
    let elf_file = ElfBytes::<AnyEndian>::minimal_parse(KERNEL)?;

    let root_page_table = create_root_page_table()?;

    println!(
        "Created root page table: {:#X}!",
        root_page_table as *const _ as usize
    );

    map_kernel(root_page_table, &elf_file)?;
    println!("Mapped kernel executable!");

    let rsp = create_kernel_stack(root_page_table)?;

    println!("Created kernel stack at {:#X}!", rsp);

    map_physical_memory(root_page_table)?;

    println!("Mapped physical memory!");

    let trampoline_addr = allocate_and_map_trampoline(root_page_table)?;

    println!("Allocated trampoline at {:#X}!", trampoline_addr as u64);

    let boot_info = write_boot_info(BootInfo { serial_base: COM1 })?;

    println!("Written boot info!");

    let memory_map = unsafe { boot::exit_boot_services(None) };

    println!("Exited boot services!");

    let entry_addr = elf_file.ehdr.e_entry;

    println!("About to jump to {entry_addr:#X}!");

    let phys_memory_start = PHYSICAL_MEMORY_START.as_u64();
    let rsp_int = rsp.as_u64();

    // unsafe { asm!("3: jmp 3b") }

    unsafe {
        asm! {
            "mov rdi, {boot_info}",
            "add rdi, {phys_start}",
            "mov rsp, {rsp}",
            "mov r10, {page_table}",
            "mov r11, {entry}",
            "jmp {trampoline}",
            boot_info = in(reg) boot_info,
            phys_start = in(reg) phys_memory_start,
            rsp = in(reg) rsp_int,
            page_table = in(reg) root_page_table,
            entry = in(reg) entry_addr,
            trampoline = in(reg) trampoline_addr,
            out("r10") _,
            out("r11") _,
            out("rdi") _,
        }
    }

    unreachable!()
}

/// Copies the trampoline into physical memory at a location low enough that it
/// does not conflict with the kernel, and then identity-map it in the kernel's
/// page table. This allows the trampoline to execute in both the bootloader's
/// address space and the kernel's.
fn allocate_and_map_trampoline(root_page_table: &mut PageTable) -> Result<*const u8> {
    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    let page_start = boot::allocate_pages(
        AllocateType::MaxAddress(PHYSICAL_MEMORY_START.as_u64()),
        MemoryType::BOOT_SERVICES_CODE,
        1,
    )?
    .as_ptr();

    unsafe {
        ptr::copy_nonoverlapping(TRAMPOLINE.as_ptr(), page_start, TRAMPOLINE.len());

        mapper.map_to(
            Page::from_start_address(VirtAddr::from_ptr(page_start))?,
            PhysFrame::from_start_address(PhysAddr::new(page_start as u64))?,
            PageTableFlags::PRESENT,
            &mut UefiFrameAllocator,
        )?;
    }

    Ok(page_start)
}

fn create_root_page_table() -> Result<&'static mut PageTable> {
    let level_4_table = IdentityFrameMapping.frame_to_pointer(
        UefiFrameAllocator
            .allocate_frame()
            .ok_or(LoaderError::InvalidMemoryMap)?,
    );
    unsafe { ptr::write(level_4_table, PageTable::new()) };
    Ok(unsafe { &mut *level_4_table })
}

/// Create a virtual memory mapping for the kernel executable
fn map_kernel(root_page_table: &mut PageTable, elf_file: &ElfBytes<AnyEndian>) -> Result<()> {
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
            let _ = unsafe {
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

/// Create a stack for the kernel. Currently this only maps a single page - the second page in the region reserved for kernel stacks.
fn create_kernel_stack(root_page_table: &mut PageTable) -> Result<VirtAddr> {
    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    let page_start = STACK_START + Size4KiB::SIZE;

    let page = Page::<Size4KiB>::from_start_address(page_start)?;

    let kernel_stack_page_address =
        boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, 1)?;
    let kernel_stack_phys_frame = PhysFrame::from_start_address(PhysAddr::new(
        kernel_stack_page_address.addr().get() as u64,
    ))?;

    unsafe {
        let _ = mapper.map_to(
            page,
            kernel_stack_phys_frame,
            PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE,
            &mut UefiFrameAllocator,
        )?;
    }

    /// Rsp points to the end of the page we have just allocated
    Ok(page_start + Size4KiB::SIZE)
}

/// Maps the physical memory in the kernel's virtual memory, at an offset
fn map_physical_memory(root_page_table: &mut PageTable) -> Result<()> {
    let types_to_map = [
        MemoryType::LOADER_CODE,
        MemoryType::LOADER_DATA,
        MemoryType::BOOT_SERVICES_CODE,
        MemoryType::BOOT_SERVICES_DATA,
        MemoryType::CONVENTIONAL,
        MemoryType::ACPI_RECLAIM,
        MemoryType::ACPI_NON_VOLATILE,
        MemoryType::MMIO,
        MemoryType::MMIO_PORT_SPACE,
        MemoryType::PAL_CODE,
        MemoryType::PERSISTENT_MEMORY,
    ];

    let memory_map = boot::memory_map(MemoryType::LOADER_DATA)?;

    let max_phys_addr = memory_map
        .entries()
        .filter(|desc| types_to_map.contains(&desc.ty))
        .map(|desc| {
            println!(
                "Type: {:?}, start: {:#X}, count: {}",
                desc.ty, desc.phys_start, desc.page_count
            );

            desc.phys_start + (desc.page_count * boot::PAGE_SIZE as u64)
        })
        .max()
        .ok_or(LoaderError::InvalidMemoryMap)?;

    println!(
        "Max physical address: {:#X}. This means the size is {} MB",
        max_phys_addr,
        max_phys_addr / 1024 / 1024
    );

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

// fn find_serial_port() -> Result<(), LoaderError> {
//     let rsdp = unsafe { acpi::find_rsdp().unwrap() };
//     let rsdt = rsdp.rsdt();

//     system::with_stdout(|out| writeln!(out, "RSDT: {:?}", rsdt))?;

//     for entry in rsdt.entries() {
//         system::with_stdout(|out| writeln!(out, "Entry: {:?}", entry))?;
//     }

//     Ok(())
// }

fn write_boot_info(boot_info: BootInfo) -> Result<*const BootInfo> {
    const _: () = {
        assert!(mem::align_of::<BootInfo>() <= 8);
    };

    let ptr = boot::allocate_pool(MemoryType::LOADER_DATA, mem::size_of::<BootInfo>())?;
    let ptr = ptr.as_ptr() as *mut BootInfo;

    unsafe { ptr::write(ptr, boot_info) };

    Ok(ptr)
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let _ = println!("Received panic: {}!", info.message());

    if let Some(location) = info.location() {
        let _ = println!("Location: {}", location);
    }

    loop {
        unsafe { asm!("hlt") }
    }

    // runtime::reset(ResetType::SHUTDOWN, Status::SUCCESS, None)
}
