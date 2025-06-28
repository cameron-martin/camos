## Useful QEMU flags

* `-d int` - Log interrupts
* `-no-reboot` - Don't reboot on triple exceptions

## Roadmap

* Set up IDT. I guess the UEFI one is still loaded currently?
* Write a page frame allocator. See https://wiki.osdev.org/Memory_Allocation for an overview of memory management, or https://wiki.osdev.org/Page_Frame_Allocation for more specific details.
* Work out how to use large pages. How does this work with UEFI, where pages are always 4KiB?