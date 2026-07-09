// src/fs/tar.rs

use alloc::string::ToString;

pub struct TarVnode {
    pub name: alloc::string::String,
    pub size: usize,
    pub data_ptr: *const u8, // Pointer to where the file data starts in RAM
}

// Ensure Rust knows it's safe to share across threads
unsafe impl Send for TarVnode {}
unsafe impl Sync for TarVnode {}

impl crate::fs::vfs::Vnode for TarVnode {
    fn vtype(&self) -> crate::fs::vfs::VnodeType {
        crate::fs::vfs::VnodeType::File
    }

    fn size(&self) -> usize {
        self.size
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Option<usize> {
        if offset >= self.size {
            return Some(0);
        }

        let bytes_to_read = core::cmp::min(buf.len(), self.size - offset);
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.data_ptr.add(offset),
                buf.as_mut_ptr(),
                bytes_to_read,
            );
        }
        Some(bytes_to_read)
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Option<usize> {
        None // Initramfs is Read-Only!
    }

    fn lookup(&self, _name: &str) -> Option<alloc::sync::Arc<dyn crate::fs::vfs::Vnode>> {
        None // Normal files don't have children
    }
}

fn parse_octal(bytes: &[u8]) -> usize {
    let mut result = 0;
    for &b in bytes {
        if b >= b'0' && b <= b'7' {
            result = result * 8 + (b - b'0') as usize;
        } else if b == 0 || b == b' ' {
            if result > 0 {
                break;
            }
        }
    }
    result
}

pub struct TarDirectory {
    pub files: alloc::vec::Vec<alloc::sync::Arc<TarVnode>>,
}

impl crate::fs::vfs::Vnode for TarDirectory {
    fn vtype(&self) -> crate::fs::vfs::VnodeType {
        crate::fs::vfs::VnodeType::Directory
    }

    fn size(&self) -> usize {
        0
    }

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Option<usize> {
        None // Directories cannot be read like normal files
    }

    fn write(&self, _offset: usize, _buf: &[u8]) -> Option<usize> {
        None // Read-only!
    }

    fn lookup(&self, name: &str) -> Option<alloc::sync::Arc<dyn crate::fs::vfs::Vnode>> {
        for i in self.files.iter() {
            if i.name == name {
                return Some(i.clone() as alloc::sync::Arc<dyn crate::fs::vfs::Vnode>);
            }
        }
        None
    }
}


pub fn parse_tarball(base_addr: u64) -> alloc::sync::Arc<TarDirectory> {
    let mut files = alloc::vec::Vec::new();
    let mut current_addr = base_addr;

    loop {
        // Create a 512-byte slice pointing to the current header block
        let header = unsafe { core::slice::from_raw_parts(current_addr as *const u8, 512) };

        if header[0] == 0 {
            break;
        }
        
        let mut null_idx = 0;
        for i in 0..100 {
            if header[i] == 0 {
                null_idx = i;
                break;
            }
        }

        let name = unsafe {
            core::str::from_utf8_unchecked(&header[0..null_idx])
        }.to_string();

        let size = parse_octal(&header[124..136]);
        

        let data_ptr = current_addr + 512;

        files.push(alloc::sync::Arc::new(TarVnode {
            name,
            size,
            data_ptr: data_ptr as *const u8,
        }));

        current_addr += 512 + crate::mm::allocator::align_to(size, 512) as u64;
        
    }

    crate::serial_println!("Parsed {} files from Initramfs", files.len());
    alloc::sync::Arc::new(TarDirectory { files })
}
