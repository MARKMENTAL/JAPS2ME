//! Copy-on-write helpers for operations that modify a card image.

use super::card::{ImportFile, MemoryCard};
use anyhow::{anyhow, Context, Result};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;

/// Progress updates emitted while an import is staged, written, and validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddProgress {
    CopyingCard {
        bytes_copied: u64,
        total_bytes: u64,
    },
    WritingSave {
        bytes_written: u64,
        total_bytes: u64,
    },
    VerifyingSave {
        files_verified: usize,
        total_files: usize,
    },
    Committing,
}

pub fn add_directory_atomically(
    card_path: impl AsRef<Path>,
    name: &str,
    directory_mode: u16,
    created: [u8; 8],
    modified: [u8; 8],
    files: &[ImportFile],
) -> Result<()> {
    add_directory_atomically_with_progress(
        card_path,
        name,
        directory_mode,
        created,
        modified,
        files,
        |_| {},
    )
}

/// Import a save atomically while reporting progress for each staging phase.
pub fn add_directory_atomically_with_progress<F>(
    card_path: impl AsRef<Path>,
    name: &str,
    directory_mode: u16,
    created: [u8; 8],
    modified: [u8; 8],
    files: &[ImportFile],
    mut report: F,
) -> Result<()>
where
    F: FnMut(AddProgress),
{
    let card_path = fs::canonicalize(card_path.as_ref())
        .with_context(|| format!("resolving card path {}", card_path.as_ref().display()))?;
    let metadata = fs::metadata(&card_path)?;
    let parent = card_path.parent().unwrap_or_else(|| Path::new("."));
    let mut temporary = NamedTempFile::new_in(parent)
        .with_context(|| format!("creating temporary image in {}", parent.display()))?;
    let total_copy_bytes = metadata.len();
    report(AddProgress::CopyingCard {
        bytes_copied: 0,
        total_bytes: total_copy_bytes,
    });
    let mut source = File::open(&card_path)?;
    let mut copied = 0u64;
    let mut buffer = [0u8; 128 * 1024];
    loop {
        let read = source.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        temporary.as_file_mut().write_all(&buffer[..read])?;
        copied += read as u64;
        report(AddProgress::CopyingCard {
            bytes_copied: copied,
            total_bytes: total_copy_bytes,
        });
    }
    ensure_copy_complete(copied, total_copy_bytes)?;
    fs::set_permissions(temporary.path(), metadata.permissions())?;

    let total_save_bytes = files.iter().try_fold(0u64, |total, file| {
        total
            .checked_add(u64::try_from(file.data.len()).context("save payload is too large")?)
            .ok_or_else(|| anyhow!("save payload size overflow"))
    })?;
    report(AddProgress::WritingSave {
        bytes_written: 0,
        total_bytes: total_save_bytes,
    });
    {
        let mut card = MemoryCard::open(temporary.path(), true)?;
        let mut bytes_written = 0u64;
        card.add_directory_with_progress(
            name,
            directory_mode,
            created,
            modified,
            files,
            |bytes| {
                bytes_written += bytes;
                report(AddProgress::WritingSave {
                    bytes_written,
                    total_bytes: total_save_bytes,
                });
            },
        )?;
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
    report(AddProgress::VerifyingSave {
        files_verified: 0,
        total_files: files.len(),
    });
    for (index, file) in files.iter().enumerate() {
        let imported = imported_entries
            .iter()
            .find(|entry| entry.name == file.name)
            .ok_or_else(|| anyhow!("staged save is missing file {}", file.name))?;
        if staged.read_file(imported)? != file.data {
            return Err(anyhow!("staged file contents differ for {}", file.name));
        }
        report(AddProgress::VerifyingSave {
            files_verified: index + 1,
            total_files: files.len(),
        });
    }
    drop(staged);

    report(AddProgress::Committing);
    temporary
        .persist(&card_path)
        .map_err(|error| error.error)
        .with_context(|| format!("replacing memory card {}", card_path.display()))?;
    if let Ok(directory) = File::open(parent) {
        let _ = directory.sync_all();
    }
    Ok(())
}

fn ensure_copy_complete(copied: u64, expected: u64) -> Result<()> {
    if copied != expected {
        return Err(anyhow!(
            "memory-card image changed while copying ({copied} of {expected} bytes copied)"
        ));
    }
    Ok(())
}
