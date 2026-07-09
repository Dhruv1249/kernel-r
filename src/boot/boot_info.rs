// src/boot/boot_info.rs

/// The common two-field header that opens every Multiboot2 tag.
///
/// GRUB prefixes every information tag with this struct. The `typ` field
/// identifies what kind of data follows (memory map, ACPI RSDP, etc.) and
/// `size` gives the total byte length of the tag including this header, which
/// is used to step to the next tag.
#[repr(C)]
pub struct TagHeader {
    pub typ: u32,
    pub size: u32,
}

/// Forward-only iterator over the Multiboot2 information structure (MBI).
///
/// GRUB places a flat array of variable-length tags starting 8 bytes after the
/// MBI base address (the first 8 bytes are the total size and a reserved word).
/// Each tag is padded to an 8-byte boundary. Iteration stops when the terminal
/// tag (`typ == 0`, `size == 8`) is reached or when the end address is passed.
pub struct TagIterator {
    current_address: usize,
    end_address: usize,
}

impl TagIterator {
    /// Creates a new iterator starting at the first tag inside the MBI.
    ///
    /// `mbi_addr` must be the physical (or identity-mapped virtual) address
    /// of the Multiboot2 information structure that GRUB passes in `ebx`/`rdi`.
    /// The first `u32` at that address is the total byte size of the MBI,
    /// used to compute the end address.
    ///
    /// # Safety
    /// The caller must guarantee that `mbi_addr` points to a valid, readable
    /// Multiboot2 information structure in memory.
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

    /// Advances the iterator and returns a raw pointer to the next tag header.
    ///
    /// After reading a tag's size field the cursor is advanced by `size` bytes
    /// then rounded up to the next 8-byte boundary, matching the Multiboot2
    /// spec's alignment rule.  Returns `None` when the terminal tag or the end
    /// of the MBI is encountered.
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


/// A single entry from the Multiboot2 memory map tag (type 6).
///
/// Each entry describes one contiguous physical memory region.  The `typ`
/// field classifies the region:
/// - `1` = usable RAM (safe to hand to the frame allocator)
/// - `2` = reserved (BIOS / firmware use)
/// - `3` = ACPI reclaimable
/// - `4` = ACPI NVS (must be preserved across S3 suspend)
/// - `5` = bad RAM
#[repr(C)]
pub struct MemoryMapEntry {
    pub base_addr: u64,
    pub length: u64,
    pub typ: u32,
    pub reserved: u32,
}

/// The header of the Multiboot2 memory-map tag (tag type 6).
///
/// Immediately follows the generic [`TagHeader`] when `typ == 6`.
/// The actual array of [`MemoryMapEntry`] structs begins 16 bytes after the
/// start of the tag (i.e., right after this struct).
#[repr(C)]
pub struct MemoryMapTag {
    pub typ: u32,
    pub size: u32,
    pub entry_size: u32,
    pub entry_version: u32,
}

/// Multiboot2 ACPI 1.0 old RSDP tag (type 14).
///
/// When GRUB detects an ACPI 1.0 system it embeds a copy of the Root System
/// Description Pointer (RSDP) directly in the MBI as this tag.  The `rsdt_address`
/// field holds the 32-bit physical address of the ACPI Root System Description
/// Table (RSDT), which is then used to locate the MADT and other ACPI tables.
///
/// `#[repr(C, packed)]` prevents the compiler from inserting padding between
/// fields, ensuring the layout exactly matches the ACPI specification.
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

/// Multiboot2 ACPI 2.0 extended RSDP tag (type 15).
///
/// ACPI 2.0 extends the RSDP with a 64-bit XSDT address (`xsdt_address`) and
/// an additional checksum covering the extended fields.  When present, the XSDT
/// should be preferred over the 32-bit RSDT because it can reference tables
/// anywhere in the 64-bit physical address space.
///
/// `#[repr(C, packed)]` prevents padding, matching the ACPI 2.0 RSDP layout.
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

/// Common header that opens every ACPI System Description Table (SDT).
///
/// All ACPI tables (RSDT, XSDT, MADT, etc.) begin with this 36-byte header.
/// The `signature` field is a 4-byte ASCII string that identifies the table
/// type (e.g. `b"APIC"` for the MADT, `b"RSDT"` for the RSDT).
/// The `checksum` byte is chosen so that the sum of all bytes in the table
/// (including the header) equals zero modulo 256.
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

/// Multiple APIC Description Table (MADT) — ACPI table with signature `"APIC"`.
///
/// The MADT describes the interrupt controller topology of the system.  It
/// begins with an [`SdtHeader`] followed by the physical MMIO address of the
/// Local APIC and a `flags` field, after which a variable-length sequence of
/// interrupt-controller records follows (parsed with [`MadtRecordHeader`]).
// Multiple APIC Description Table (MADT)
#[repr(C, packed)]
pub struct Madt {
    pub header: SdtHeader,
    pub local_apic_address: u32, // The physical MMIO address of the Local APIC
    pub flags: u32,
    // Variable-length interrupt controller structures follow here...
}

/// Two-byte header common to every record inside the MADT.
///
/// After the fixed [`Madt`] fields, the table contains a packed sequence of
/// interrupt-controller records.  Each record starts with this header:
/// `entry_type` identifies the record kind and `record_length` gives the total
/// byte length of the record (including this header), enabling safe iteration
/// over records of unknown or future types.
#[repr(C, packed)]
pub struct MadtRecordHeader {
    pub entry_type: u8,
    pub record_length: u8,
}

/// MADT record type 1: I/O APIC descriptor.
///
/// When `entry_type == 1` the record describes one I/O APIC in the system.
/// `io_apic_address` is the 32-bit physical MMIO base address of that I/O APIC,
/// and `global_system_interrupt_base` is the first global interrupt number it
/// handles (usually 0 for the first I/O APIC).
#[repr(C, packed)]
pub struct IoApicRecord {
    pub header: MadtRecordHeader,
    pub io_apic_id: u8,
    pub reserved: u8,
    pub io_apic_address: u32, // The physical MMIO address!
    pub global_system_interrupt_base: u32,
}



/// Multiboot2 Module Tag (Type 3)
#[repr(C, packed)]
pub struct ModuleTag {
    pub typ: u32,
    pub size: u32,
    pub mod_start: u32,
    pub mod_end: u32,
}
