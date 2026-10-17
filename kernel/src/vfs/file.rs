use alloc::vec::Vec;

#[derive(Clone, Copy)]
pub struct FileMetadata {
    pub inode: u64,
    pub mode: u16,
    pub uid: u32,
    pub gid: u32,
    pub links: u32,
    pub size: u64,
    pub blocks: u64,
    pub atime: u32,
    pub mtime: u32,
    pub ctime: u32,
    pub btime: u32,
}

#[derive(Clone, Copy)]
pub enum AccessMode {
    ReadOnly,
    WriteOnly,
    ReadWrite,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory,
}

#[derive(Clone, Copy)]
pub struct VfsNode {
    pub metadata: FileMetadata,
    pub kind: NodeKind,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VfsHandle(pub(super) usize);

#[derive(Clone)]
pub struct DirectoryEntry {
    pub inode: u64,
    pub file_type: u8,
    pub name: Vec<u8>,
}
