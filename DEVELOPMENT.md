## Useful QEMU flags

* `-d int` - Log interrupts
* `-no-reboot` - Don't reboot on triple exceptions

## Roadmap

* Write a kernel heap allocator (see the bonwick slab allocator paper).
* Set up IDT. I guess the UEFI one is still loaded currently?
* Work out how to use large pages. How does this work with UEFI, where pages are always 4KiB?