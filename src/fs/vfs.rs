/// Represents the type of a Vnode
pub enum VnodeType {
    File,
    Directory,
    CharacterDevice, // For things like stdin/stdout/serial
}

pub static ROOT_FS: spin::Mutex<Option<alloc::sync::Arc<dyn Vnode>>> = spin::Mutex::new(None);

/// The core trait that all filesystems must implement.
pub trait Vnode: Send + Sync {
    fn vtype(&self) -> VnodeType;
    fn size(&self) -> usize;
    
    /// Reads up to `buf.len()` bytes starting at `offset`. 
    /// Returns the number of bytes read.
    fn read(&self, offset: usize, buf: &mut [u8]) -> Option<usize>;
    
    /// Writes up to `buf.len()` bytes starting at `offset`.
    /// Returns the number of bytes written.
    fn write(&self, offset: usize, buf: &[u8]) -> Option<usize>;
    
    /// For directories: looks up a child node by name.
    fn lookup(&self, name: &str) -> Option<alloc::sync::Arc<dyn Vnode>>;
}

/// Represents an opened file and its current state (like the read/write offset).
pub struct OpenFile {
    pub vnode: alloc::sync::Arc<dyn Vnode>,
    pub offset: usize,
    pub readable: bool,
    pub writable: bool,
}

pub struct ConsoleVnode;

impl Vnode for ConsoleVnode {
    fn vtype(&self) -> VnodeType {
        VnodeType::CharacterDevice
    }

    fn size(&self) -> usize {
        0 // Character devices don't have a file size
    }

    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Option<usize> {
        None // We will implement keyboard reading later
    }

    fn write(&self, _offset: usize, buf: &[u8]) -> Option<usize> {
        let s = unsafe { core::str::from_utf8_unchecked(buf) };
        crate::serial_print!("{}", s);
        crate::println!("{}", s);
        return Some(buf.len());
    }

    fn lookup(&self, _name: &str) -> Option<alloc::sync::Arc<dyn Vnode>> {
        None // Consoles don't have child files
    }
}
