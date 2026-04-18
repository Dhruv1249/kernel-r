// src/boot_info.rs

#[repr(C)]
pub struct TagHeader {
    pub typ: u32,
    pub size: u32,
}

pub struct TagIterator {
    current_address: usize,
    end_address: usize,
}

impl TagIterator {
    pub fn new(mbi_addr: usize) -> Self {
        let total_size = unsafe { *(mbi_addr as *const u32) };
        TagIterator {
            current_address: mbi_addr + 8,
            end_address: mbi_addr + total_size as usize,
        }
    }
}

impl Iterator for TagIterator {
    type Item = *const TagHeader;
    fn next(&mut self) -> Option<Self::Item> {
        if self.current_address >= self.end_address {
            return None;
        }
        let tag_ptr = self.current_address as *const TagHeader;
        let tag_header = unsafe { &*tag_ptr };
        
        // The terminal tag tells us we are done
        if tag_header.typ == 0 && tag_header.size == 8 {
            return None;
        }
        // 1. Advance the pointer by the exact size of the current tag
        let next_addr = self.current_address + tag_header.size as usize;

        // 2. Round up to the nearest 8-byte boundary
        self.current_address = (next_addr + 7) & !7;

        Some(tag_ptr)
    }
}


#[repr(C)]
pub struct MemoryMapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub typ: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct MemoryMapTag {
    pub typ: u32,
    pub size: u32,
    pub entry_size: u32,
    pub entry_version: u32,
}
