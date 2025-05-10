use core::{fmt, mem, ptr::addr_of, slice, str};

use uefi::{system, table::cfg};

/// The signature of an ACPI header. This represents 4 chars, e.g. "ECDT".
#[repr(transparent)]
pub struct AcpiFixedString<const N: usize>([u8; N]);

impl<const N: usize> fmt::Debug for AcpiFixedString<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(str::from_utf8(&self.0).unwrap())
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct AcpiRsdp {
    pub signature: AcpiFixedString<8>,
    pub checksum: u8,
    pub oem_id: AcpiFixedString<6>,
    pub revision: u8,
    pub rsdt_address: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct AcpiTableHeader {
    pub signature: AcpiFixedString<4>,
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: AcpiFixedString<6>,
    pub oem_table_id: AcpiFixedString<8>,
    pub oem_revision: u32,
    pub creator_id: [u8; 4],
    pub creator_revision: [u8; 4],
}

/// A root system descriptor table
#[repr(C)]
#[derive(Debug)]
pub struct AcpiRsdt {
    pub header: AcpiTableHeader,
    // Dynamic number of entries
}

#[derive(Debug)]
pub enum AcpiTable<'a> {
    Rsdt(&'a AcpiRsdt),
    Unknown(&'a AcpiTableHeader),
}

impl<'a> AcpiTable<'a> {
    /// "Parses" an ACPI table, by looking at the signature and casting to a more specific type.
    pub fn parse(table: &'a AcpiTableHeader) -> Self {
        match &table.signature.0 {
            b"RSDT" => Self::Rsdt(unsafe { &*(table as *const _ as *const AcpiRsdt) }),
            _ => Self::Unknown(table)
        }
    }
}

impl AcpiRsdt {
    /// An array of 32-bit physical addresses that point to other DESCRIPTION_HEADERs.
    pub fn entry_addrs(&self) -> &[u32] {
        let n = (self.header.length as usize - mem::size_of::<AcpiTableHeader>()) / 4;

        unsafe {
            slice::from_raw_parts(addr_of!(self.header).add(1) as *const u32, n)
        }
    }

    pub fn entries(&self) -> impl Iterator<Item = AcpiTable> {
        self.entry_addrs().iter().map(|addr| AcpiTable::parse(unsafe { &*(*addr as *const AcpiTableHeader) }))
    }
}

impl AcpiRsdp {
    pub fn rsdt(&self) -> &AcpiRsdt {
        unsafe {
            &*(self.rsdt_address as *const AcpiRsdt)
        }
    }
}

/// Finds the root system descriptor pointer, using UEFI
/// 
/// # Safety
/// 
/// It is up to the caller to ensure the returned lifetime is correct
/// 
pub unsafe fn find_rsdp<'a>() -> Option<&'a AcpiRsdp> {
    let rsdp_addr = system::with_config_table(|entries| {
        entries
            .iter()
            .find(|entry| entry.guid == cfg::ACPI2_GUID)
            .map(|entry| entry.address)
    })?;

    // assert!()

    Some(&*(rsdp_addr as *const AcpiRsdp))
}
