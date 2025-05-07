#![no_std]
#![no_main]

use core::{fmt::Write, panic::PanicInfo};

use elf::{ElfBytes, ParseError, abi::PT_LOAD, endian::AnyEndian};
use thiserror::Error;
use uefi::{
    boot::{self, AllocateType, MemoryDescriptor, MemoryType},
    mem::memory_map::{MemoryMap, MemoryMapOwned},
    prelude::entry,
    system,
};

static KERNEL: &[u8] = include_bytes!(env!("KERNEL_PATH"));

#[derive(Error, Debug)]
enum LoaderError {
    #[error("cannot parse elf: {0}")]
    ElfParse(#[from] ParseError),
    #[error("the kernel elf was invalid")]
    InvalidKernelElf,
    #[error("invalid memory map")]
    InvalidMemoryMap,
}

#[entry]
fn efi_main() -> uefi::Status {
    let Ok(_) = system::with_stdout(|out| writeln!(out, "Hello from Rust!")) else {
        return uefi::Status::WARN_WRITE_FAILURE;
    };

    // TODO: Allocate the OS page tables before entering boot services!
    boot::allocate_pages(AllocateType::AnyPages, MemoryType::LOADER_DATA, count);

    let memory_map = unsafe { boot::exit_boot_services(None) };

    load_kernel(memory_map);

    uefi::Status::SUCCESS
}

/// Create a virtual memory mapping for the kernel executable then jumps to it
fn load_kernel(memory_map: MemoryMapOwned) -> Result<(), LoaderError> {
    // This is where the OS page tables will go
    let first_physical_memory_section = memory_map
        .entries()
        .filter(|descriptor| descriptor.ty == MemoryType::CONVENTIONAL)
        .next()
        .ok_or(LoaderError::InvalidMemoryMap)?;

    let elf_file = ElfBytes::<AnyEndian>::minimal_parse(KERNEL)?;
    let segments = elf_file.segments().ok_or(LoaderError::InvalidKernelElf)?;

    let load_segments = segments
        .into_iter()
        .filter(|segment| segment.p_type == PT_LOAD);

    for segment in load_segments {
        let offset = segment.p_offset as usize;
        let len = segment.p_filesz as usize;
        let segment_bytes = &KERNEL[offset..offset + len];

        let virt_start = segment.p_vaddr;
        let virt_end = virt_start + segment.p_memsz;

        let permissions = permissions_from_flags(segment.flags());

        for frame in frame(phys_start)..=frame(phys_end - 1) {
            // TODO: create page table mapping for frame with permissions
            // at corresponding virtual address
        }

        // if virt_end > phys_end {
        //     // TODO: there is a `.bss` section in this segment -> map next
        //     // (virt_end - phys_end) bytes to free physical frame and initialize
        //     // them with zero
        // }
    }

    let entry_addr = elf_file.ehdr.e_entry;

    // Jump to entry_addr
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}
