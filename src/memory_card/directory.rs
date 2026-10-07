//! PS2 memory-card directory entries.

use anyhow::{anyhow, Result};

pub const DIRENT_SIZE: usize = 512;
pub const DF_READ: u16 = 0x0001;
pub const DF_WRITE: u16 = 0x0002;
pub const DF_EXECUTE: u16 = 0x0004;
pub const DF_RWX: u16 = DF_READ | DF_WRITE | DF_EXECUTE;
pub const DF_FILE: u16 = 0x0010;
pub const DF_DIR: u16 = 0x0020;
pub const DF_EXISTS: u16 = 0x8000;

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub mode: u16,
    pub length: u32,
    pub created: [u8; 8],
    pub first_cluster: u32,
    pub parent_index: u32,
    pub modified: [u8; 8],
    pub attr: u32,
    pub name: String,
}

impl DirEntry {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < DIRENT_SIZE {
            return Err(anyhow!("directory entry too short"));
        }
        let mode = u16::from_le_bytes([data[0], data[1]]);
        let length = u32::from_le_bytes(data[4..8].try_into()?);
        let mut created = [0; 8];
        created.copy_from_slice(&data[8..16]);
        let first_cluster = u32::from_le_bytes(data[16..20].try_into()?);
        let parent_index = u32::from_le_bytes(data[20..24].try_into()?);
        let mut modified = [0; 8];
        modified.copy_from_slice(&data[24..32]);
        let attr = u32::from_le_bytes(data[32..36].try_into()?);
        let end = data[64..512].iter().position(|&b| b == 0).unwrap_or(448);
        let name = String::from_utf8_lossy(&data[64..64 + end]).to_string();
        Ok(Self {
            mode,
            length,
            created,
            first_cluster,
            parent_index,
            modified,
            attr,
            name,
        })
    }

    pub fn to_bytes(&self) -> [u8; DIRENT_SIZE] {
        let mut out = [0u8; DIRENT_SIZE];
        out[0..2].copy_from_slice(&self.mode.to_le_bytes());
        out[4..8].copy_from_slice(&self.length.to_le_bytes());
        out[8..16].copy_from_slice(&self.created);
        out[16..20].copy_from_slice(&self.first_cluster.to_le_bytes());
        out[20..24].copy_from_slice(&self.parent_index.to_le_bytes());
        out[24..32].copy_from_slice(&self.modified);
        out[32..36].copy_from_slice(&self.attr.to_le_bytes());
        let name = self.name.as_bytes();
        let n = name.len().min(447);
        out[64..64 + n].copy_from_slice(&name[..n]);
        out
    }

    pub fn is_dir(&self) -> bool {
        self.mode & (DF_DIR | DF_EXISTS) == (DF_DIR | DF_EXISTS)
    }
    pub fn is_file(&self) -> bool {
        self.mode & (DF_FILE | DF_EXISTS) == (DF_FILE | DF_EXISTS)
    }
}
