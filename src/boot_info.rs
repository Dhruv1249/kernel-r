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

// We don't want compiler to add padding bytes
#[repr(C, packed)]
pub struct AcpiV1Tag {
    pub typ: u32,
    pub size: u32,
    // --- RSDP Structure begins here ---
    pub signature: [u8; 8], // "RSD PTR "
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,  // The physical address of the RSDT!
}

// We don't want compiler to add padding bytes
#[repr(C, packed)]
pub struct AcpiV2Tag {
    pub typ: u32,
    pub size: u32,
    // --- RSDP Version 2 Structure begins here ---
    pub signature: [u8; 8],
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,  // Kept for backward compatibility
    pub length: u32,
    pub xsdt_address: u64,  // <- This is the one we actually want!
    pub extended_checksum: u8,
    pub reserved: [u8; 3],
}

#[repr(C, packed)]
pub struct SdtHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

// Multiple APIC Description Table (MADT)
#[repr(C, packed)]
pub struct Madt {
    pub header: SdtHeader,
    pub local_apic_address: u32, // The physical MMIO address of the Local APIC
    pub flags: u32,
    // Variable-length interrupt controller structures follow here...
}

#[repr(C, packed)]
pub struct MadtRecordHeader {
    pub entry_type: u8,
    pub record_length: u8,
}

#[repr(C, packed)]
pub struct IoApicRecord {
    pub header: MadtRecordHeader,
    pub io_apic_id: u8,
    pub reserved: u8,
    pub io_apic_address: u32, // The physical MMIO address!
    pub global_system_interrupt_base: u32,
}
