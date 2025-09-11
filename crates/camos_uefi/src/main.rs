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

/// The start address of the region of memory reserved by the kernel. This
/// extends until the end of the whole physical address space. Regions below
/// this can be used to share memory between the
const KERNEL_START: VirtAddr = PHYSICAL_MEMORY_START;

/// The address where physical memory is mapped.
const PHYSICAL_MEMORY_START: VirtAddr = VirtAddr::new(0xFFFF_8800_0000_0000);
const PHYSICAL_MEMORY_END: VirtAddr = KERNEL_EXECUTABLE_START;

/// The address of the start of the kernel executable
const KERNEL_EXECUTABLE_START: VirtAddr = VirtAddr::new(0xFFFF_FFFF_8000_0000);
const KERNEL_EXECUTABLE_END: VirtAddr = STACK_START;

/// The address of the start of the region of virtual memory containing kernel
/// stacks. This does not include the initial kernel stack, which is allocated
/// in the lower half of the address space.
const STACK_START: VirtAddr = VirtAddr::new(0xFFFF_FFFF_9000_0000);

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

    let boot_info = write_and_map_boot_info(
        root_page_table,
        BootInfo {
            serial_base: COM1,
            physical_offset: PHYSICAL_MEMORY_START,
            memory_map: None,
            pages: 0,
        },
    )?;

    println!(
        "Written boot info to address {:#X}!",
        boot_info as *const _ as usize
    );

    exit_boot_services(boot_info, root_page_table)?;

    println!("Exited boot services!");

    let entry_addr = elf_file.ehdr.e_entry;

    println!("About to jump to {entry_addr:#X}!");

    let phys_memory_start = PHYSICAL_MEMORY_START.as_u64();
    let rsp_int = rsp.as_u64();

    // unsafe { asm!("3: jmp 3b") }

    unsafe {
        asm! {
            "mov rsp, {rsp}",
            "jmp {trampoline}",
            in("rdi") boot_info,
            in("r10") root_page_table,
            in("r11") entry_addr,
            rsp = in(reg) rsp_int,
            trampoline = in(reg) trampoline_addr,
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
        AllocateType::MaxAddress(KERNEL_START.as_u64()),
        MemoryType::BOOT_SERVICES_CODE,
        1,
    )?
    .as_ptr();

    unsafe {
        ptr::copy_nonoverlapping(TRAMPOLINE.as_ptr(), page_start, TRAMPOLINE.len());

        let _ = mapper.map_to(
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

    let segments = elf_file.segments().ok_or(LoaderError::InvalidKernelElf)?;

    let load_segments = segments
        .into_iter()
        .filter(|segment| segment.p_type == PT_LOAD);

    for segment in load_segments {
        // The offset into the page where this segment needs to be copied, to
        // get the correct virtual address.
        let page_offset = segment.p_vaddr % Size4KiB::SIZE;
        let vaddr_page_start = VirtAddr::new(segment.p_vaddr - page_offset);

        // The number of pages that need to be allocated to copy this segment into
        let n_pages = (segment.p_memsz + page_offset).div_ceil(Size4KiB::SIZE);

        println!("Allocating {} pages for elf segment", n_pages);

        // TODO: Don't always allocate this as LOADER_CODE!
        let allocated_pages = boot::allocate_pages(
            AllocateType::AnyPages,
            MemoryType::LOADER_CODE,
            n_pages as usize,
        )?
        .as_ptr();

        unsafe {
            ptr::copy_nonoverlapping(
                KERNEL[segment.p_offset as usize..].as_ptr(),
                allocated_pages.add(page_offset as usize),
                segment.p_filesz as usize,
            );
        }

        // If the size in memory is large than the size in the elf, pad with zeros.
        if segment.p_memsz > segment.p_filesz {
            unsafe {
                ptr::write_bytes(
                    allocated_pages
                        .add(page_offset as usize)
                        .add(segment.p_filesz as usize),
                    0,
                    (segment.p_memsz - segment.p_filesz) as usize,
                );
            }
        }

        if vaddr_page_start < KERNEL_EXECUTABLE_START
            || vaddr_page_start + (n_pages * Size4KiB::SIZE) > KERNEL_EXECUTABLE_END
        {
            panic!(
                "The kernel contains a segment that is outside of the virtual address range reserved for the kernel code and data"
            );
        }

        for i in 0..n_pages {
            let page =
                Page::<Size4KiB>::from_start_address(vaddr_page_start + (i * Size4KiB::SIZE))?;
            let frame = PhysFrame::<Size4KiB>::from_start_address(
                PhysAddr::new(allocated_pages as u64) + (i * Size4KiB::SIZE),
            )?;

            println!(
                "Mapping page starting at virtual address {:#X} to physical address {:#X}",
                page.start_address(),
                frame.start_address(),
            );

            let _ = unsafe {
                mapper.map_to(
                    page,
                    frame,
                    PageTableFlags::PRESENT | PageTableFlags::GLOBAL,
                    &mut frame_allocator,
                )
            }?;
        }
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

    // Rsp points to the end of the page we have just allocated
    Ok(page_start + Size4KiB::SIZE)
}

/// Maps the physical memory in the kernel's virtual memory, at a fixed offset
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

    if virt_range.end.start_address() + Size4KiB::SIZE > PHYSICAL_MEMORY_END {
        panic!(
            "The size of the physical memory exceeds the size of the region in virtual memory reserved for the physical memory map"
        );
    }

    let phys_range = PhysFrame::<Size4KiB>::range_inclusive(
        PhysFrame::containing_address(PhysAddr::new(0)),
        PhysFrame::containing_address(PhysAddr::new(max_phys_addr) - 1),
    );

    for (page, phys_frame) in virt_range.zip(phys_range) {
        let _ = unsafe {
            mapper.map_to(
                page,
                phys_frame,
                PageTableFlags::PRESENT | PageTableFlags::GLOBAL | PageTableFlags::WRITABLE,
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

fn write_and_map_boot_info(
    root_page_table: &mut PageTable,
    boot_info: BootInfo,
) -> Result<&'static mut BootInfo> {
    let pages_for_boot_info = mem::size_of::<BootInfo>().div_ceil(boot::PAGE_SIZE);

    let ptr = boot::allocate_pages(
        AllocateType::MaxAddress(KERNEL_START.as_u64()),
        MemoryType::LOADER_DATA,
        pages_for_boot_info,
    )?;
    let ptr = ptr.as_ptr() as *mut BootInfo;

    // Identity-map this into the kernel's page table
    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    unsafe {
        let _ = mapper.map_to(
            Page::from_start_address(VirtAddr::from_ptr(ptr))?,
            PhysFrame::from_start_address(PhysAddr::new(ptr as u64))?,
            PageTableFlags::PRESENT,
            &mut UefiFrameAllocator,
        )?;
    }

    unsafe { ptr::write(ptr, boot_info) };

    let boot_info = unsafe { &mut *ptr };
    boot_info.pages = pages_for_boot_info;

    Ok(boot_info)
}

/// Exits boot services, obtaining the memory map and making this available to
/// the kernel via BootInfo, ensuring that it is mapped into the kernel's
/// address space.
fn exit_boot_services(boot_info: &mut BootInfo, root_page_table: &mut PageTable) -> Result<()> {
    let memory_map = unsafe { boot::exit_boot_services(None) };

    let mut mapper = unsafe { MappedPageTable::new(root_page_table, IdentityFrameMapping) };

    let memory_map_range = memory_map.buffer().as_ptr_range();

    for page in Page::<Size4KiB>::range_inclusive(
        Page::containing_address(VirtAddr::from_ptr(memory_map_range.start)),
        Page::containing_address(VirtAddr::from_ptr(memory_map_range.end) - 1),
    ) {
        // Check that this page doesn't conflict with the kernel's address
        // space. This isn't guaranteed to be true. For this we'd have to ask
        // UEFI to allocate the memory map at an address below a fixed value,
        // but until we do this hope for the best.
        assert!(page.start_address() + page.size() <= KERNEL_START);

        unsafe {
            let _ = mapper.map_to(
                page,
                PhysFrame::from_start_address(PhysAddr::new(page.start_address().as_u64()))?,
                PageTableFlags::PRESENT,
                &mut UefiFrameAllocator,
            );
        }
    }

    boot_info.memory_map = Some(memory_map);

    Ok(())
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
}
