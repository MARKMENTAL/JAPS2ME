//! Copy-on-write helpers for operations that modify a card image.

use super::card::{ImportFile, MemoryCard};
use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::path::Path;
use tempfile::NamedTempFile;

pub fn add_directory_atomically(
    card_path: impl AsRef<Path>,
    name: &str,
    directory_mode: u16,
    created: [u8; 8],
    modified: [u8; 8],
    files: &[ImportFile],
) -> Result<()> {
    let card_path = fs::canonicalize(card_path.as_ref())
        .with_context(|| format!("resolving card path {}", card_path.as_ref().display()))?;
    let metadata = fs::metadata(&card_path)?;
    let parent = card_path.parent().unwrap_or_else(|| Path::new("."));
    let temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary image in {}", parent.display()))?;
    fs::copy(&card_path, temporary.path())?;
    fs::set_permissions(temporary.path(), metadata.permissions())?;

    {
        let mut card = MemoryCard::open(temporary.path(), true)?;
        card.add_directory(name, directory_mode, created, modified, files)?;
    }
    File::open(temporary.path())?.sync_all()?;

    // Validate the staged card after closing the writer, before replacing the
    // original image.
    let mut staged = MemoryCard::open(temporary.path(), false)?;
    let save_entry = staged
        .entries()?
        .into_iter()
        .find(|entry| entry.name == name)
        .ok_or_else(|| anyhow!("staged memory card is missing imported save {name}"))?;
    let imported_entries = staged.directory_entries(save_entry.first_cluster, save_entry.length)?;
    for file in files {
        let imported = imported_entries
            .iter()
            .find(|entry| entry.name == file.name)
            .ok_or_else(|| anyhow!("staged save is missing file {}", file.name))?;
        if staged.read_file(imported)? != file.data {
            return Err(anyhow!("staged file contents differ for {}", file.name));
        }
    }
    drop(staged);

    temporary
        .persist(&card_path)
        .map_err(|error| error.error)
        .with_context(|| format!("replacing memory card {}", card_path.display()))?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}
