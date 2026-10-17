mod file;
mod openfd;
mod dev;

use alloc::{string::String, sync::Arc, vec, vec::Vec};

use ext4_rs::Ext4;
use spin::Once;

use crate::{
    fs,
    lock::IntMutex,
    process::{Process, fd::{Errno, FileOps}},
};

pub use file::{AccessMode, DirectoryEntry, FileMetadata, NodeKind, VfsHandle, VfsNode};

pub const BLOCK_SIZE: usize = ext4_rs::BLOCK_SIZE;

pub fn is_builtin_device(path: &str) -> bool {
    dev::is_device(path)
}

pub fn resolve_path(cwd: &str, path: &str) -> Result<String, Errno> {
    if path.is_empty() {
        return Err(Errno::ENOENT);
    }

    let mut components = Vec::<&str>::new();
    let inputs: &[&str] = if path.starts_with('/') {
        &[path]
    } else {
        &[cwd, path]
    };
    for input in inputs {
        for component in input.split('/') {
            match component {
                "" | "." => {}
                ".." => {
                    components.pop();
                }
                component => {
                    components.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
                    components.push(component);
                }
            }
        }
    }

    let mut length = 1usize;
    for component in &components {
        length = length
            .checked_add(component.len())
            .and_then(|length| length.checked_add(1))
            .ok_or(Errno::ENAMETOOLONG)?;
    }
    let mut resolved = String::new();
    resolved.try_reserve(length).map_err(|_| Errno::ENOMEM)?;
    resolved.push('/');
    for (index, component) in components.iter().enumerate() {
        if index != 0 {
            resolved.push('/');
        }
        resolved.push_str(component);
    }
    Ok(resolved)
}

pub enum VfsRequest {
    Lookup {
        path: String,
    },
    Open {
        path: String,
        access: AccessMode,
    },
    Metadata {
        path: String,
    },
    ReadAll {
        path: String,
    },
    Read {
        handle: VfsHandle,
        offset: usize,
        length: usize,
    },
    Write {
        handle: VfsHandle,
        offset: usize,
        data: Vec<u8>,
    },
    ReadDir {
        handle: VfsHandle,
        position: usize,
    },
    Size {
        handle: VfsHandle,
    },
    Close {
        handle: VfsHandle,
    },
}

pub enum VfsResponse {
    Node(VfsNode),
    Opened { handle: VfsHandle, kind: NodeKind },
    Metadata(FileMetadata),
    Data(Vec<u8>),
    Written(usize),
    DirectoryEntry(Option<DirectoryEntry>),
    Size(usize),
    Closed,
}

enum Ext4Open {
    File { fs: Arc<Ext4>, inode: u32 },
    Directory { entries: Vec<DirectoryEntry> },
}

#[derive(Default)]
struct Ext4OpenTable {
    entries: Vec<Option<Ext4Open>>,
}

static OPEN_FILES: Once<IntMutex<Ext4OpenTable>> = Once::new();

fn open_files() -> &'static IntMutex<Ext4OpenTable> {
    OPEN_FILES.call_once(|| IntMutex::new(Ext4OpenTable::default()));
    OPEN_FILES.get().unwrap()
}

fn insert_open(open: Ext4Open) -> Result<VfsHandle, Errno> {
    let mut opens = open_files().lock();
    if let Some(index) = opens.entries.iter().position(Option::is_none) {
        opens.entries[index] = Some(open);
        return Ok(VfsHandle(index));
    }
    opens.entries.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
    let handle = VfsHandle(opens.entries.len());
    opens.entries.push(Some(open));
    Ok(handle)
}

fn remove_open(handle: VfsHandle) -> Result<(), Errno> {
    let mut opens = open_files().lock();
    let entry = opens.entries.get_mut(handle.0).ok_or(Errno::EBADF)?;
    entry.take().ok_or(Errno::EBADF)?;
    while opens.entries.last().is_some_and(Option::is_none) {
        opens.entries.pop();
    }
    Ok(())
}

pub fn request(request: VfsRequest) -> Result<VfsResponse, Errno> {
    match request {
        VfsRequest::Lookup { path } => {
            if let Some(node) = dev::node(&path) {
                return node.map(VfsResponse::Node);
            }
            let (fs, inode) = lookup(&path)?;
            Ok(VfsResponse::Node(node(&fs, inode)))
        }
        VfsRequest::Open { path, access } => {
            if path == "/dev" {
                if !matches!(access, AccessMode::ReadOnly) {
                    return Err(Errno::EROFS);
                }
                let handle = insert_open(Ext4Open::Directory { entries: dev::directory_entries() })?;
                return Ok(VfsResponse::Opened { handle, kind: NodeKind::Directory });
            }
            let (fs, inode) = lookup(&path)?;
            let inode_ref = fs.get_inode_ref(inode);
            let (open, kind) = if inode_ref.inode.is_dir() {
                if !matches!(access, AccessMode::ReadOnly) {
                    return Err(Errno::EROFS);
                }
                let mut entries: Vec<DirectoryEntry> = fs.dir_get_entries(inode)
                    .into_iter()
                    .map(|entry| DirectoryEntry {
                        inode: entry.inode as u64,
                        file_type: entry.get_de_type(),
                        name: entry.name[..entry.get_name_len()].to_vec(),
                    })
                    .collect();
                if path == "/" && !entries.iter().any(|entry| entry.name == b"dev") {
                    entries.push(dev::root_entry());
                }
                (Ext4Open::Directory { entries }, NodeKind::Directory)
            } else if inode_ref.inode.is_file() {
                if !matches!(access, AccessMode::ReadOnly) {
                    return Err(Errno::EROFS);
                }
                (Ext4Open::File { fs, inode }, NodeKind::File)
            } else {
                return Err(Errno::ENOENT);
            };
            let handle = insert_open(open)?;
            Ok(VfsResponse::Opened { handle, kind })
        }
        VfsRequest::Metadata { path } => {
            if let Some(node) = dev::node(&path) {
                return node.map(|node| VfsResponse::Metadata(node.metadata));
            }
            let (fs, inode) = lookup(&path)?;
            Ok(VfsResponse::Metadata(node(&fs, inode).metadata))
        }
        VfsRequest::ReadAll { path } => {
            let (fs, inode) = lookup(&path)?;
            let inode_ref = fs.get_inode_ref(inode);
            if !inode_ref.inode.is_file() {
                return Err(Errno::EISDIR);
            }
            let size = usize::try_from(inode_ref.inode.size()).map_err(|_| Errno::EOVERFLOW)?;
            let mut data = vec![0; size];
            let count = fs.read_at(inode, 0, &mut data).map_err(|_| Errno::EIO)?;
            data.truncate(count);
            Ok(VfsResponse::Data(data))
        }
        VfsRequest::Read {
            handle,
            offset,
            length,
        } => {
            let opens = open_files().lock();
            let Some(Ext4Open::File { fs, inode }) =
                opens.entries.get(handle.0).and_then(Option::as_ref)
            else {
                return Err(Errno::EBADF);
            };
            let mut data = vec![0; length];
            let count = fs
                .read_at(*inode, offset, &mut data)
                .map_err(|_| Errno::EIO)?;
            data.truncate(count);
            Ok(VfsResponse::Data(data))
        }
        VfsRequest::Write {
            handle,
            offset,
            data,
        } => {
            let _ = (handle, offset, data);
            Err(Errno::EROFS)
        }
        VfsRequest::ReadDir { handle, position } => {
            let opens = open_files().lock();
            let Some(Ext4Open::Directory { entries }) =
                opens.entries.get(handle.0).and_then(Option::as_ref)
            else {
                return Err(Errno::ENOTDIR);
            };
            Ok(VfsResponse::DirectoryEntry(entries.get(position).cloned()))
        }
        VfsRequest::Size { handle } => {
            let opens = open_files().lock();
            let Some(Ext4Open::File { fs, inode }) =
                opens.entries.get(handle.0).and_then(Option::as_ref)
            else {
                return Err(Errno::EBADF);
            };
            let size = usize::try_from(fs.get_inode_ref(*inode).inode.size())
                .map_err(|_| Errno::EOVERFLOW)?;
            Ok(VfsResponse::Size(size))
        }
        VfsRequest::Close { handle } => {
            remove_open(handle)?;
            Ok(VfsResponse::Closed)
        }
    }
}

fn lookup(path: &str) -> Result<(Arc<Ext4>, u32), Errno> {
    let fs = fs::root().ok_or(Errno::ENOENT)?;
    let inode = if path.is_empty() || path == "/" {
        fs::ROOT_INODE
    } else {
        let mut parent = fs::ROOT_INODE;
        fs.generic_open(path, &mut parent, false, 0, &mut 0)
            .map_err(|_| Errno::ENOENT)?
    };
    Ok((fs, inode))
}

fn node(fs: &Ext4, inode: u32) -> VfsNode {
    let inode_ref = fs.get_inode_ref(inode);
    let inode = inode_ref.inode;
    VfsNode {
        kind: if inode.is_dir() {
            NodeKind::Directory
        } else {
            NodeKind::File
        },
        metadata: FileMetadata {
            inode: inode_ref.inode_num as u64,
            mode: inode.mode,
            uid: inode.uid as u32 | ((inode.osd2.l_i_uid_high as u32) << 16),
            gid: inode.gid as u32 | ((inode.osd2.l_i_gid_high as u32) << 16),
            links: inode.links_count as u32,
            size: inode.size(),
            blocks: inode.blocks as u64 | ((inode.osd2.l_i_blocks_high as u64) << 32),
            atime: inode.atime,
            mtime: inode.mtime,
            ctime: inode.ctime,
            btime: inode.i_crtime,
        },
    }
}

pub fn open_file(proc: &Process, path: &str, access: AccessMode) -> Result<Arc<dyn FileOps>, Errno> {
    if let Some(target) = dev::open(proc, path, access) {
        return target;
    }
    match request(VfsRequest::Open {
        path: String::from(path),
        access,
    })? {
        VfsResponse::Opened { handle, kind } => Ok(Arc::new(openfd::OpenFD::new(handle, kind))),
        _ => Err(Errno::EIO),
    }
}

pub fn metadata(path: &str) -> Result<FileMetadata, Errno> {
    match request(VfsRequest::Metadata {
        path: String::from(path),
    })? {
        VfsResponse::Metadata(metadata) => Ok(metadata),
        _ => Err(Errno::EIO),
    }
}

pub fn read_all(path: &str) -> Result<Vec<u8>, Errno> {
    match request(VfsRequest::ReadAll {
        path: String::from(path),
    })? {
        VfsResponse::Data(data) => Ok(data),
        _ => Err(Errno::EIO),
    }
}

struct LinuxDirent64<'a> {
    inode: u64,
    next_offset: i64,
    file_type: u8,
    name: &'a [u8],
}

impl LinuxDirent64<'_> {
    const HEADER_LEN: usize = 19;

    fn encode(&self) -> Vec<u8> {
        let record_len = (Self::HEADER_LEN + self.name.len() + 1 + 7) & !7;
        let mut bytes = vec![0u8; record_len];
        bytes[0..8].copy_from_slice(&self.inode.to_le_bytes());
        bytes[8..16].copy_from_slice(&self.next_offset.to_le_bytes());
        bytes[16..18].copy_from_slice(&(record_len as u16).to_le_bytes());
        bytes[18] = self.file_type;
        bytes[Self::HEADER_LEN..Self::HEADER_LEN + self.name.len()].copy_from_slice(self.name);
        bytes
    }
}
