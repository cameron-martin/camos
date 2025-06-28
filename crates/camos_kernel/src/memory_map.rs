//! Provides an abstraction over memory maps, for testing purposes

use uefi::{
    boot::MemoryDescriptor,
    mem::memory_map::{MemoryMap as UefiMemoryMap, MemoryMapOwned},
};

pub trait MemoryMap {
    fn entries(&self) -> impl Iterator<Item = &MemoryDescriptor>;
}

impl MemoryMap for MemoryMapOwned {
    fn entries(&self) -> impl Iterator<Item = &MemoryDescriptor> {
        UefiMemoryMap::entries(self)
    }
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
pub struct TestMemoryMap {
    entries: std::vec::Vec<MemoryDescriptor>,
}

#[cfg(test)]
impl TestMemoryMap {
    pub fn new(entries: std::vec::Vec<MemoryDescriptor>) -> Self {
        Self { entries }
    }
}

#[cfg(test)]
impl MemoryMap for TestMemoryMap {
    fn entries(&self) -> impl Iterator<Item = &MemoryDescriptor> {
        self.entries.iter()
    }
}
