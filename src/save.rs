//! Shared model and format detection for PS2 save imports.

use anyhow::{bail, Context, Result};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct SaveFile {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u16,
    pub created: [u8; 8],
    pub modified: [u8; 8],
}

#[derive(Debug, Clone)]
pub struct SaveImport {
    pub directory: String,
    pub directory_mode: u16,
    pub created: [u8; 8],
    pub modified: [u8; 8],
    pub title: String,
    pub files: Vec<SaveFile>,
}

impl SaveImport {
    /// Parse a supported save container by its magic bytes.
    pub fn parse(data: &[u8]) -> Result<Self> {
        if data.starts_with(b"CFU\0") {
            crate::codebreaker::format::parse(data)
        } else if data.starts_with(b"Ps2PowerSave") {
            crate::max_drive::parse(data)
        } else {
            bail!("unrecognized save format (expected Codebreaker .cbs or MAX Drive .max)");
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&data).with_context(|| format!("parsing {}", path.display()))
    }
}
