use super::directory::{DirEntry, DF_DIR, DF_EXISTS, DF_FILE, DF_RWX, DIRENT_SIZE};
use super::ecc::page_spare;
use super::fat::{ALLOCATED, CHAIN_END, CHAIN_END_UNALLOC};
use super::icon_sys::parse_title;
use super::superblock::Superblock;
use crate::save::{SaveFile, SaveImport};
use anyhow::{anyhow, bail, ensure, Context, Result};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use time::{OffsetDateTime, UtcOffset};

const DF_0400: u16 = 0x0400;

#[derive(Debug, Clone)]
pub struct ImportFile {
    pub name: String,
    pub data: Vec<u8>,
    pub mode: u16,
    pub created: [u8; 8],
    pub modified: [u8; 8],
}

/// Human-readable summary of one save directory on the card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveSummary {
    pub directory: String,
    pub game_title: String,
    pub file_count: usize,
    pub size_bytes: u64,
}

pub struct MemoryCard {
    file: File,
    pub path: PathBuf,
    pub superblock: Superblock,
    has_ecc: bool,
}

impl MemoryCard {
    pub fn open(path: impl AsRef<Path>, write: bool) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut file = OpenOptions::new()
            .read(true)
            .write(write)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        let mut data = [0u8; 512];
        file.read_exact(&mut data)?;
        let superblock = Superblock::from_bytes(&data)?;
        ensure!(superblock.page_size > 0, "invalid memory-card page size");
        ensure!(
            superblock.pages_per_cluster > 0,
            "invalid pages per cluster"
        );

        let pages = u64::from(superblock.clusters_per_card)
            .checked_mul(u64::from(superblock.pages_per_cluster))
            .ok_or_else(|| anyhow!("memory-card page count overflow"))?;
        let data_len = pages * u64::from(superblock.page_size);
        let ecc_len = pages * u64::from(superblock.raw_page_size());
        let image_len = file.metadata()?.len();
        let has_ecc = if image_len >= ecc_len {
            true
        } else if image_len >= data_len {
            false
        } else {
            bail!(
                "memory-card image is truncated ({image_len} bytes; expected at least {data_len})"
            );
        };
        Ok(Self {
            file,
            path,
            superblock,
            has_ecc,
        })
    }

    fn read_cluster(&mut self, cluster: u32) -> Result<Vec<u8>> {
        ensure!(
            cluster < self.superblock.allocatable_cluster_end,
            "cluster out of range: {cluster}"
        );
        self.read_raw_cluster(cluster + self.superblock.allocatable_cluster_offset)
    }

    fn write_cluster(&mut self, cluster: u32, data: &[u8]) -> Result<()> {
        ensure!(
            cluster < self.superblock.allocatable_cluster_end,
            "cluster out of range: {cluster}"
        );
        self.write_raw_cluster(cluster + self.superblock.allocatable_cluster_offset, data)
    }

    fn fat_entry(&mut self, cluster: u32) -> Result<u32> {
        ensure!(
            cluster < self.superblock.allocatable_cluster_end,
            "FAT cluster out of range: {cluster}"
        );
        let entries_per_cluster = self.superblock.entries_per_cluster();
        let fat_cluster_index = cluster / entries_per_cluster;
        let entry_index = (cluster % entries_per_cluster) as usize;
        let indirect_index = (fat_cluster_index / entries_per_cluster) as usize;
        ensure!(
            indirect_index < self.superblock.indirect_fat_cluster_list.len(),
            "indirect FAT index out of range"
        );
        let indirect_entry_index = (fat_cluster_index % entries_per_cluster) as usize;

        let indirect_cluster = self.superblock.indirect_fat_cluster_list[indirect_index];
        let indirect = self.read_raw_cluster(indirect_cluster)?;
        let fat_cluster = read_u32(&indirect, indirect_entry_index * 4)?;
        let fat = self.read_raw_cluster(fat_cluster)?;
        read_u32(&fat, entry_index * 4)
    }

    fn set_fat(&mut self, cluster: u32, value: u32) -> Result<()> {
        ensure!(
            cluster < self.superblock.allocatable_cluster_end,
            "FAT cluster out of range: {cluster}"
        );
        let entries_per_cluster = self.superblock.entries_per_cluster();
        let fat_cluster_index = cluster / entries_per_cluster;
        let entry_index = (cluster % entries_per_cluster) as usize;
        let indirect_index = (fat_cluster_index / entries_per_cluster) as usize;
        ensure!(
            indirect_index < self.superblock.indirect_fat_cluster_list.len(),
            "indirect FAT index out of range"
        );
        let indirect_entry_index = (fat_cluster_index % entries_per_cluster) as usize;

        let indirect_cluster = self.superblock.indirect_fat_cluster_list[indirect_index];
        let indirect = self.read_raw_cluster(indirect_cluster)?;
        let fat_cluster = read_u32(&indirect, indirect_entry_index * 4)?;
        let mut fat = self.read_raw_cluster(fat_cluster)?;
        write_u32(&mut fat, entry_index * 4, value)?;
        self.write_raw_cluster(fat_cluster, &fat)
    }

    fn read_raw_cluster(&mut self, cluster: u32) -> Result<Vec<u8>> {
        let page_size = self.superblock.page_size as usize;
        let pages_per_cluster = self.superblock.pages_per_cluster as usize;
        let stride = if self.has_ecc {
            self.superblock.raw_page_size() as u64
        } else {
            self.superblock.page_size as u64
        };
        let first_page = u64::from(cluster) * pages_per_cluster as u64;
        let mut output = Vec::with_capacity(page_size * pages_per_cluster);
        for page_index in 0..pages_per_cluster {
            let offset = (first_page + page_index as u64) * stride;
            self.file.seek(SeekFrom::Start(offset))?;
            let mut page = vec![0; page_size];
            self.file.read_exact(&mut page)?;
            output.extend_from_slice(&page);
            if self.has_ecc {
                self.file.seek(SeekFrom::Start(offset + page_size as u64))?;
                let mut spare = vec![0; self.superblock.spare_size() as usize];
                self.file.read_exact(&mut spare)?;
            }
        }
        Ok(output)
    }

    fn write_raw_cluster(&mut self, cluster: u32, data: &[u8]) -> Result<()> {
        let page_size = self.superblock.page_size as usize;
        let pages_per_cluster = self.superblock.pages_per_cluster as usize;
        ensure!(
            data.len() == page_size * pages_per_cluster,
            "cluster write has incorrect length"
        );
        let stride = if self.has_ecc {
            self.superblock.raw_page_size() as u64
        } else {
            self.superblock.page_size as u64
        };
        let first_page = u64::from(cluster) * pages_per_cluster as u64;
        for page_index in 0..pages_per_cluster {
            let page = &data[page_index * page_size..(page_index + 1) * page_size];
            let offset = (first_page + page_index as u64) * stride;
            self.file.seek(SeekFrom::Start(offset))?;
            self.file.write_all(page)?;
            if self.has_ecc {
                let spare = page_spare(page, self.superblock.spare_size() as usize);
                self.file.write_all(&spare)?;
            }
        }
        Ok(())
    }

    fn chain(&mut self, first: u32) -> Result<Vec<u32>> {
        let mut chain = Vec::new();
        let mut visited = HashSet::new();
        let mut current = first;
        while current != CHAIN_END {
            ensure!(
                current < self.superblock.allocatable_cluster_end,
                "FAT chain points outside allocatable area"
            );
            ensure!(visited.insert(current), "FAT cycle at cluster {current}");
            chain.push(current);
            let next = self.fat_entry(current)?;
            if next == CHAIN_END {
                break;
            }
            ensure!(
                next & ALLOCATED != 0,
                "invalid FAT link at cluster {current}"
            );
            current = next & !ALLOCATED;
        }
        Ok(chain)
    }

    pub fn read_file(&mut self, entry: &DirEntry) -> Result<Vec<u8>> {
        if entry.length == 0 {
            return Ok(Vec::new());
        }
        let mut data = Vec::new();
        for cluster in self.chain(entry.first_cluster)? {
            data.extend_from_slice(&self.read_cluster(cluster)?);
        }
        data.truncate(entry.length as usize);
        Ok(data)
    }

    fn allocate_cluster(&mut self) -> Result<u32> {
        for cluster in 1..self.superblock.allocatable_cluster_end {
            if self.fat_entry(cluster)? & ALLOCATED == 0 {
                self.set_fat(cluster, CHAIN_END)?;
                return Ok(cluster);
            }
        }
        bail!("memory card is full")
    }

    fn free_chain(&mut self, first: u32) -> Result<()> {
        let chain = self.chain(first)?;
        for cluster in chain {
            self.set_fat(cluster, CHAIN_END_UNALLOC)?;
        }
        Ok(())
    }

    fn write_file_data<F>(&mut self, data: &[u8], payload_progress: &mut F) -> Result<u32>
    where
        F: FnMut(u64),
    {
        if data.is_empty() {
            return Ok(CHAIN_END);
        }
        let cluster_size = self.superblock.cluster_size() as usize;
        let mut first = CHAIN_END;
        let mut previous = None;
        for chunk in data.chunks(cluster_size) {
            let cluster = self.allocate_cluster()?;
            if first == CHAIN_END {
                first = cluster;
            }
            if let Some(previous) = previous {
                self.set_fat(previous, cluster | ALLOCATED)?;
            }
            let mut padded = vec![0; cluster_size];
            padded[..chunk.len()].copy_from_slice(chunk);
            self.write_cluster(cluster, &padded)?;
            payload_progress(chunk.len() as u64);
            previous = Some(cluster);
        }
        Ok(first)
    }

    pub fn directory_entries(&mut self, first_cluster: u32, length: u32) -> Result<Vec<DirEntry>> {
        let chain = self.chain(first_cluster)?;
        let entries_per_cluster = self.superblock.cluster_size() as usize / DIRENT_SIZE;
        let capacity = chain.len() * entries_per_cluster;
        ensure!(
            length as usize <= capacity,
            "directory length exceeds its FAT chain capacity"
        );
        let mut entries = Vec::with_capacity(length as usize);
        for cluster in chain {
            let data = self.read_cluster(cluster)?;
            for chunk in data.chunks_exact(DIRENT_SIZE) {
                if entries.len() == length as usize {
                    return Ok(entries);
                }
                entries.push(DirEntry::from_bytes(chunk)?);
            }
        }
        Ok(entries)
    }

    fn write_directory_entry(
        &mut self,
        first_cluster: u32,
        index: usize,
        entry: &DirEntry,
    ) -> Result<()> {
        let entries_per_cluster = self.superblock.cluster_size() as usize / DIRENT_SIZE;
        let chain = self.chain(first_cluster)?;
        let cluster_index = index / entries_per_cluster;
        ensure!(
            cluster_index < chain.len(),
            "directory entry index exceeds allocated chain"
        );
        let cluster = chain[cluster_index];
        let local_index = index % entries_per_cluster;
        let mut data = self.read_cluster(cluster)?;
        let offset = local_index * DIRENT_SIZE;
        data[offset..offset + DIRENT_SIZE].copy_from_slice(&entry.to_bytes());
        self.write_cluster(cluster, &data)
    }

    fn ensure_directory_capacity(&mut self, first_cluster: u32, index: usize) -> Result<()> {
        let entries_per_cluster = self.superblock.cluster_size() as usize / DIRENT_SIZE;
        let needed = index / entries_per_cluster + 1;
        let mut chain = self.chain(first_cluster)?;
        while chain.len() < needed {
            let new_cluster = self.allocate_cluster()?;
            let previous = *chain
                .last()
                .ok_or_else(|| anyhow!("directory has no starting cluster"))?;
            self.set_fat(previous, new_cluster | ALLOCATED)?;
            self.write_cluster(
                new_cluster,
                &vec![0; self.superblock.cluster_size() as usize],
            )?;
            chain.push(new_cluster);
        }
        Ok(())
    }

    fn append_directory_entry(
        &mut self,
        first_cluster: u32,
        entry_count: u32,
        is_root: bool,
        entry: &DirEntry,
    ) -> Result<(u32, u32)> {
        let mut dot = self
            .directory_entries(first_cluster, 2)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("directory is missing its . entry"))?;
        ensure!(entry_count >= 2, "directory entry count is invalid");
        let entries = self.directory_entries(first_cluster, entry_count)?;
        let free_index = entries
            .iter()
            .enumerate()
            .skip(2)
            .find_map(|(index, current)| (current.mode & DF_EXISTS == 0).then_some(index));
        let index = free_index.unwrap_or(entry_count as usize);
        self.ensure_directory_capacity(first_cluster, index)?;
        self.write_directory_entry(first_cluster, index, entry)?;
        let new_entry_count = if index == entry_count as usize {
            entry_count
                .checked_add(1)
                .ok_or_else(|| anyhow!("directory is too large"))?
        } else {
            entry_count
        };

        // For the root directory there is no parent dirent holding its
        // length, so the root '.' entry carries that count. For save
        // directories the count belongs to the save's root dirent; their
        // own '.' entry must retain its reference-format length of zero.
        if is_root {
            dot.length = new_entry_count;
            dot.modified = current_tod();
            self.write_directory_entry(first_cluster, 0, &dot)?;
        }
        Ok((index as u32, new_entry_count))
    }

    pub fn entries(&mut self) -> Result<Vec<DirEntry>> {
        let root_dot = self
            .directory_entries(0, 1)?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("root directory is empty"))?;
        Ok(self
            .directory_entries(0, root_dot.length)?
            .into_iter()
            .skip(2)
            .filter(|entry| entry.mode & DF_EXISTS != 0)
            .collect())
    }

    /// Summarize each save directory with its game title and payload size.
    pub fn save_summaries(&mut self) -> Result<Vec<SaveSummary>> {
        let root_entries = self.entries()?;
        let mut summaries = Vec::new();
        for save in root_entries.into_iter().filter(|entry| entry.is_dir()) {
            let children = self.directory_entries(save.first_cluster, save.length)?;
            let files: Vec<&DirEntry> = children.iter().filter(|entry| entry.is_file()).collect();
            let mut size_bytes = 0u64;
            for file in &files {
                size_bytes = size_bytes
                    .checked_add(u64::from(file.length))
                    .ok_or_else(|| anyhow!("save size overflow"))?;
            }
            let game_title = self.icon_title(&children, &save.name);
            summaries.push(SaveSummary {
                directory: save.name.clone(),
                game_title,
                file_count: files.len(),
                size_bytes,
            });
        }
        Ok(summaries)
    }

    fn icon_title(&mut self, children: &[DirEntry], fallback: &str) -> String {
        children
            .iter()
            .filter(|entry| entry.is_file())
            .find(|file| file.name.eq_ignore_ascii_case("icon.sys"))
            .filter(|icon| icon.length == 964)
            .and_then(|icon| self.read_file(icon).ok())
            .as_deref()
            .and_then(parse_title)
            .unwrap_or_else(|| fallback.to_owned())
    }

    /// Read a save directory into the shared save model for exporting.
    pub fn export_save(&mut self, name: &str) -> Result<SaveImport> {
        let save = self
            .entries()?
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| anyhow!("save not found: {name}"))?;
        ensure!(save.is_dir(), "not a save directory: {name}");
        let children = self.directory_entries(save.first_cluster, save.length)?;
        let mut files = Vec::new();
        for child in children.iter().skip(2) {
            if child.mode & DF_EXISTS == 0 {
                continue;
            }
            if child.is_dir() {
                bail!("refusing to export nested directory {}", child.name);
            }
            ensure!(
                child.is_file(),
                "unsupported directory entry: {}",
                child.name
            );
            let data = self.read_file(child)?;
            ensure!(
                data.len() == child.length as usize,
                "truncated file data: {}",
                child.name
            );
            files.push(SaveFile {
                name: child.name.clone(),
                data,
                mode: child.mode,
                created: child.created,
                modified: child.modified,
            });
        }
        let title = self.icon_title(&children, &save.name);
        Ok(SaveImport {
            directory: save.name.clone(),
            directory_mode: save.mode,
            created: save.created,
            modified: save.modified,
            title,
            files,
        })
    }

    /// Free allocatable space in bytes.
    pub fn available_bytes(&mut self) -> Result<u64> {
        let mut free_clusters = 0u64;
        for cluster in 0..self.superblock.allocatable_cluster_end {
            if self.fat_entry(cluster)? & ALLOCATED == 0 {
                free_clusters += 1;
            }
        }
        free_clusters
            .checked_mul(u64::from(self.superblock.cluster_size()))
            .ok_or_else(|| anyhow!("free-space calculation overflow"))
    }

    /// Total allocatable space in bytes.
    pub fn total_bytes(&self) -> Result<u64> {
        u64::from(self.superblock.allocatable_cluster_end)
            .checked_mul(u64::from(self.superblock.cluster_size()))
            .ok_or_else(|| anyhow!("total-space calculation overflow"))
    }

    #[cfg(test)]
    pub(crate) fn add_directory(
        &mut self,
        name: &str,
        directory_mode: u16,
        created: [u8; 8],
        modified: [u8; 8],
        files: &[ImportFile],
    ) -> Result<()> {
        self.add_directory_with_progress(name, directory_mode, created, modified, files, |_| {})
    }

    pub(crate) fn add_directory_with_progress<F>(
        &mut self,
        name: &str,
        directory_mode: u16,
        created: [u8; 8],
        modified: [u8; 8],
        files: &[ImportFile],
        mut payload_progress: F,
    ) -> Result<()>
    where
        F: FnMut(u64),
    {
        validate_name(name)?;
        ensure!(directory_mode & DF_DIR != 0, "save mode is not a directory");
        if self.entries()?.iter().any(|entry| entry.name == name) {
            bail!("save already exists: {name}");
        }
        for file in files {
            validate_name(&file.name)?;
            u32::try_from(file.data.len()).context("file is too large for PS2 directory entry")?;
        }
        let mut names = HashSet::new();
        for file in files {
            ensure!(
                names.insert(file.name.as_str()),
                "duplicate file in save: {}",
                file.name
            );
        }

        let mut directory_entries = vec![0; self.superblock.cluster_size() as usize];
        let now = current_tod();
        let first_directory_cluster = self.allocate_cluster()?;
        let dot = DirEntry {
            mode: DF_RWX | DF_DIR | DF_0400 | DF_EXISTS,
            length: 0,
            created: now,
            // mymc+ stores the parent directory location in the first entry.
            first_cluster: 0,
            parent_index: 0,
            modified: now,
            attr: 0,
            name: ".".to_owned(),
        };
        let dotdot = DirEntry {
            mode: DF_RWX | DF_DIR | DF_0400 | DF_EXISTS,
            length: 0,
            created: now,
            first_cluster: 0,
            parent_index: 0,
            modified: now,
            attr: 0,
            name: "..".to_owned(),
        };
        directory_entries[..DIRENT_SIZE].copy_from_slice(&dot.to_bytes());
        directory_entries[DIRENT_SIZE..DIRENT_SIZE * 2].copy_from_slice(&dotdot.to_bytes());
        self.write_cluster(first_directory_cluster, &directory_entries)?;

        let mut directory_entry_count = 2u32;
        for file in files {
            let first_file_cluster = self.write_file_data(&file.data, &mut payload_progress)?;
            let file_entry = DirEntry {
                mode: (file.mode & !DF_DIR) | DF_FILE | DF_EXISTS,
                length: u32::try_from(file.data.len())
                    .context("file is too large for PS2 directory entry")?,
                created: valid_tod_or_now(file.created),
                first_cluster: first_file_cluster,
                parent_index: 0,
                modified: valid_tod_or_now(file.modified),
                attr: 0,
                name: file.name.clone(),
            };
            let (_, new_count) = self.append_directory_entry(
                first_directory_cluster,
                directory_entry_count,
                false,
                &file_entry,
            )?;
            directory_entry_count = new_count;
        }

        let root_entry = DirEntry {
            mode: (directory_mode & !DF_FILE) | DF_DIR | DF_EXISTS,
            length: directory_entry_count,
            created: valid_tod_or_now(created),
            first_cluster: first_directory_cluster,
            parent_index: 0,
            modified: valid_tod_or_now(modified),
            attr: 0,
            name: name.to_owned(),
        };
        let root_length = self.directory_entries(0, 1)?[0].length;
        let (root_index, _) = self.append_directory_entry(0, root_length, true, &root_entry)?;
        let mut dot = self.directory_entries(first_directory_cluster, 2)?[0].clone();
        dot.first_cluster = 0;
        dot.parent_index = root_index;
        self.write_directory_entry(first_directory_cluster, 0, &dot)?;
        self.file.sync_all()?;
        Ok(())
    }

    pub fn remove(&mut self, name: &str) -> Result<()> {
        let root_entries = self.entries()?;
        let target = root_entries
            .into_iter()
            .find(|entry| entry.name == name)
            .ok_or_else(|| anyhow!("save not found: {name}"))?;
        if target.is_dir() {
            for child in self
                .directory_entries(target.first_cluster, target.length)?
                .into_iter()
                .skip(2)
            {
                if child.mode & DF_EXISTS == 0 {
                    continue;
                }
                if child.is_dir() {
                    bail!("refusing to remove nested directory {}", child.name);
                }
                self.free_chain(child.first_cluster)?;
            }
        } else {
            self.free_chain(target.first_cluster)?;
        }
        if target.is_dir() {
            self.free_chain(target.first_cluster)?;
        }

        let root_length = self.directory_entries(0, 1)?[0].length;
        let root = self.directory_entries(0, root_length)?;
        let index = root
            .iter()
            .position(|entry| entry.name == name && entry.mode & DF_EXISTS != 0)
            .ok_or_else(|| anyhow!("save not found: {name}"))?;
        let mut deleted = root[index].clone();
        deleted.mode &= !DF_EXISTS;
        self.write_directory_entry(0, index, &deleted)?;
        self.file.sync_all()?;
        Ok(())
    }
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + 4)
        .ok_or_else(|| anyhow!("truncated FAT cluster"))?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

fn write_u32(data: &mut [u8], offset: usize, value: u32) -> Result<()> {
    let bytes = data
        .get_mut(offset..offset + 4)
        .ok_or_else(|| anyhow!("truncated FAT cluster"))?;
    bytes.copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "directory entry name cannot be empty");
    ensure!(
        name.len() <= 32,
        "PS2 save names must be at most 32 bytes: {name}"
    );
    ensure!(name.is_ascii(), "PS2 save names must be ASCII: {name}");
    ensure!(
        !name
            .bytes()
            .any(|byte| byte == 0 || byte == b'/' || byte == b'\\'),
        "invalid character in PS2 save name: {name}"
    );
    ensure!(
        name != "." && name != "..",
        "reserved PS2 directory name: {name}"
    );
    Ok(())
}

fn current_tod() -> [u8; 8] {
    let now = OffsetDateTime::now_utc().to_offset(UtcOffset::from_hms(9, 0, 0).unwrap());
    let mut tod = [0; 8];
    tod[1] = now.second();
    tod[2] = now.minute();
    tod[3] = now.hour();
    tod[4] = now.day();
    tod[5] = now.month() as u8;
    tod[6..8].copy_from_slice(&(now.year() as u16).to_le_bytes());
    tod
}

fn valid_tod_or_now(tod: [u8; 8]) -> [u8; 8] {
    if tod[1..].iter().all(|&byte| byte == 0) {
        current_tod()
    } else {
        tod
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_test_card(path: &Path, root_has_second_cluster: bool) {
        let page_size = 512usize;
        let pages_per_cluster = 2usize;
        let cluster_size = page_size * pages_per_cluster;
        let clusters = 128u32;
        let alloc_offset = 4u32;
        let alloc_end = 100u32;
        let mut image = vec![0u8; clusters as usize * cluster_size];
        let superblock = &mut image[..512];
        superblock[..28].copy_from_slice(b"Sony PS2 Memory Card Format ");
        superblock[28..40].copy_from_slice(b"1.2.0.0\0\0\0\0\0");
        put_u16(superblock, 40, page_size as u16);
        put_u16(superblock, 42, pages_per_cluster as u16);
        put_u16(superblock, 44, 16);
        put_u32(superblock, 48, clusters);
        put_u32(superblock, 52, alloc_offset);
        put_u32(superblock, 56, alloc_end);
        put_u32(superblock, 60, 0);
        put_u32(superblock, 64, 12);
        put_u32(superblock, 68, 11);
        put_u32(superblock, 80, 2); // indirect FAT cluster
        put_u32(superblock, 84, 0xffff_ffff);
        put_u32(superblock, 208, 0xffff_ffff);

        let indirect_start = 2 * cluster_size;
        put_u32(&mut image, indirect_start, 3); // FAT cluster
        let fat_start = 3 * cluster_size;
        for cluster in 0..alloc_end as usize {
            put_u32(&mut image, fat_start + cluster * 4, CHAIN_END_UNALLOC);
        }
        put_u32(
            &mut image,
            fat_start,
            if root_has_second_cluster {
                ALLOCATED | 1
            } else {
                CHAIN_END
            },
        );
        put_u32(&mut image, fat_start + 4, CHAIN_END);

        let root_first = alloc_offset as usize * cluster_size;
        let dot = DirEntry {
            mode: DF_RWX | DF_DIR | DF_EXISTS,
            length: if root_has_second_cluster { 4 } else { 2 },
            created: [0; 8],
            first_cluster: 0,
            parent_index: 1,
            modified: [0; 8],
            attr: 0,
            name: ".".into(),
        };
        let dotdot = DirEntry {
            mode: DF_RWX | DF_DIR | DF_EXISTS,
            length: 0,
            created: [0; 8],
            first_cluster: 0,
            parent_index: 0,
            modified: [0; 8],
            attr: 0,
            name: "..".into(),
        };
        image[root_first..root_first + DIRENT_SIZE].copy_from_slice(&dot.to_bytes());
        image[root_first + DIRENT_SIZE..root_first + 2 * DIRENT_SIZE]
            .copy_from_slice(&dotdot.to_bytes());
        if root_has_second_cluster {
            let second_start = (alloc_offset as usize + 1) * cluster_size;
            let first_existing = DirEntry {
                mode: DF_RWX | DF_DIR | DF_EXISTS,
                length: 2,
                created: [0; 8],
                first_cluster: 10,
                parent_index: 0,
                modified: [0; 8],
                attr: 0,
                name: "EXISTING".into(),
            };
            image[second_start..second_start + DIRENT_SIZE]
                .copy_from_slice(&first_existing.to_bytes());
        }
        fs::write(path, image).unwrap();
    }

    fn put_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn put_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn sample_file(name: &str, data: Vec<u8>) -> ImportFile {
        ImportFile {
            name: name.into(),
            data,
            mode: DF_FILE | DF_RWX | DF_EXISTS,
            created: [0; 8],
            modified: [0; 8],
        }
    }

    fn sample_icon_sys(first: &[u8], second: &[u8], split: u16) -> Vec<u8> {
        let mut data = vec![0u8; 964];
        data[0..4].copy_from_slice(b"PS2D");
        data[6..8].copy_from_slice(&split.to_le_bytes());
        let split = split as usize;
        for byte in data[192..192 + 68].iter_mut() {
            *byte = b' ';
        }
        data[192..192 + first.len().min(split)].copy_from_slice(&first[..first.len().min(split)]);
        let second_start = 192 + split;
        let second_len = second.len().min(68 - split);
        data[second_start..second_start + second_len].copy_from_slice(&second[..second_len]);
        data[second_start + second_len] = 0;
        data
    }

    #[test]
    fn imports_directory_and_multicluster_files() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, true);
        let payload = (0..2500).map(|n| (n % 251) as u8).collect::<Vec<_>>();
        let files = vec![
            sample_file("SAVE.DAT", payload.clone()),
            sample_file("EMPTY", vec![]),
        ];

        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        let root = card.entries()?;
        let save = root
            .iter()
            .find(|entry| entry.name == "BASLUS-TEST")
            .unwrap();
        assert!(save.is_dir());
        assert_eq!(save.length, 4);
        let children = card.directory_entries(save.first_cluster, save.length)?;
        assert_eq!(children[0].name, ".");
        assert_eq!(children[0].length, 0);
        assert_eq!(children[1].name, "..");
        assert_eq!(children[1].length, 0);
        assert_eq!(children[0].first_cluster, 0);
        let file_entry = children
            .iter()
            .find(|entry| entry.name == "SAVE.DAT")
            .unwrap();
        assert_eq!(file_entry.length as usize, payload.len());
        assert_eq!(card.read_file(file_entry)?, payload);
        let empty = children.iter().find(|entry| entry.name == "EMPTY").unwrap();
        assert_eq!(empty.length, 0);
        assert_eq!(empty.first_cluster, CHAIN_END);
        Ok(())
    }

    #[test]
    fn import_uses_a_free_root_slot_in_later_cluster() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, true);
        let files = vec![sample_file("DATA", vec![1, 2, 3])];
        MemoryCard::open(&path, true)?.add_directory("NEW-SAVE", 0x8420, [0; 8], [0; 8], &files)?;
        let mut card = MemoryCard::open(&path, false)?;
        assert!(card.entries()?.iter().any(|entry| entry.name == "NEW-SAVE"));
        Ok(())
    }

    #[test]
    fn rejects_duplicate_names_without_changing_image() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let before = fs::read(&path)?;
        let files = vec![sample_file("DATA", vec![1])];
        let result = MemoryCard::open(&path, true)?
            .add_directory("NEW-SAVE", 0x8420, [0; 8], [0; 8], &files);
        assert!(result.is_ok());
        let before_duplicate = fs::read(&path)?;
        let duplicate = MemoryCard::open(&path, true)?
            .add_directory("NEW-SAVE", 0x8420, [0; 8], [0; 8], &files);
        assert!(duplicate.is_err());
        assert_eq!(fs::read(&path)?, before_duplicate);
        assert_ne!(before, before_duplicate);
        Ok(())
    }

    #[test]
    fn atomic_import_keeps_original_on_out_of_space() -> Result<()> {
        use crate::memory_card::transaction::add_directory_atomically;

        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let original = fs::read(&path)?;
        let files = vec![sample_file("TOO-LARGE", vec![0x5a; 200 * 1024])];

        let result = add_directory_atomically(&path, "NEW-SAVE", 0x8420, [0; 8], [0; 8], &files);
        assert!(result.is_err());
        assert_eq!(fs::read(&path)?, original);
        Ok(())
    }

    #[test]
    fn atomic_import_reports_progress_through_commit() -> Result<()> {
        use crate::memory_card::transaction::{
            add_directory_atomically_with_progress, AddProgress,
        };

        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let card_size = fs::metadata(&path)?.len();
        let payload = vec![0x5a; 2500];
        let files = vec![sample_file("DATA", payload.clone())];
        let mut events = Vec::new();

        add_directory_atomically_with_progress(
            &path,
            "NEW-SAVE",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
            |event| events.push(event),
        )?;

        assert_eq!(
            events.first(),
            Some(&AddProgress::CopyingCard {
                bytes_copied: 0,
                total_bytes: card_size,
            })
        );
        assert!(events.contains(&AddProgress::CopyingCard {
            bytes_copied: card_size,
            total_bytes: card_size,
        }));
        assert!(events.contains(&AddProgress::WritingSave {
            bytes_written: payload.len() as u64,
            total_bytes: payload.len() as u64,
        }));
        assert!(events.contains(&AddProgress::VerifyingSave {
            files_verified: 1,
            total_files: 1,
        }));
        assert_eq!(events.last(), Some(&AddProgress::Committing));
        Ok(())
    }

    #[test]
    fn summaries_report_payload_size_and_fallback_title() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let files = vec![
            sample_file("SAVE.DAT", vec![1; 1500]),
            sample_file("EMPTY", vec![]),
        ];
        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        let summaries = card.save_summaries()?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].directory, "BASLUS-TEST");
        assert_eq!(summaries[0].game_title, "BASLUS-TEST");
        assert_eq!(summaries[0].file_count, 2);
        assert_eq!(summaries[0].size_bytes, 1500);
        Ok(())
    }

    #[test]
    fn summaries_use_icon_sys_title() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let icon = sample_icon_sys(b"Test Game", b"Save 1", 16);
        let files = vec![
            sample_file("icon.sys", icon),
            sample_file("DATA.BIN", vec![7; 10]),
        ];
        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        let summaries = card.save_summaries()?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].game_title, "Test Game Save 1");
        assert_eq!(summaries[0].file_count, 2);
        assert_eq!(summaries[0].size_bytes, 964 + 10);
        Ok(())
    }

    #[test]
    fn summaries_ignore_invalid_icon_sys() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let files = vec![sample_file("icon.sys", vec![0u8; 100])];
        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        let summaries = card.save_summaries()?;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].game_title, "BASLUS-TEST");
        assert_eq!(summaries[0].size_bytes, 100);
        Ok(())
    }

    #[test]
    fn export_save_round_trips_card_data() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let icon = sample_icon_sys(b"Test Game", b"Save 1", 16);
        let payload = vec![1, 2, 3, 4, 5];
        let files = vec![
            sample_file("icon.sys", icon),
            sample_file("DATA.BIN", payload.clone()),
        ];
        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        let save = card.export_save("BASLUS-TEST")?;
        assert_eq!(save.directory, "BASLUS-TEST");
        assert_eq!(save.title, "Test Game Save 1");
        assert_eq!(save.files.len(), 2);
        let data = save
            .files
            .iter()
            .find(|file| file.name == "DATA.BIN")
            .unwrap();
        assert_eq!(data.data, payload);

        let encoded = crate::codebreaker::format::encode(&save)?;
        let parsed = crate::save::SaveImport::parse(&encoded)?;
        assert_eq!(parsed.directory, "BASLUS-TEST");
        assert_eq!(parsed.title, "Test Game Save 1");
        assert_eq!(parsed.files.len(), 2);
        assert_eq!(
            parsed
                .files
                .iter()
                .find(|file| file.name == "DATA.BIN")
                .unwrap()
                .data,
            payload
        );
        Ok(())
    }

    #[test]
    fn export_save_rejects_missing_and_nested_saves() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let files = vec![sample_file("DATA", vec![1, 2, 3])];
        MemoryCard::open(&path, true)?.add_directory(
            "BASLUS-TEST",
            0x8420,
            [0; 8],
            [0; 8],
            &files,
        )?;

        let mut card = MemoryCard::open(&path, false)?;
        assert!(card.export_save("NO-SUCH-SAVE").is_err());
        drop(card);

        let mut card = MemoryCard::open(&path, true)?;
        let save = card
            .entries()?
            .into_iter()
            .find(|entry| entry.name == "BASLUS-TEST")
            .unwrap();
        let children = card.directory_entries(save.first_cluster, save.length)?;
        let index = children
            .iter()
            .position(|entry| entry.name == "DATA")
            .unwrap();
        let mut nested = children[index].clone();
        nested.mode = DF_RWX | DF_DIR | DF_EXISTS;
        card.write_directory_entry(save.first_cluster, index, &nested)?;
        drop(card);

        let mut card = MemoryCard::open(&path, false)?;
        assert!(card.export_save("BASLUS-TEST").is_err());
        Ok(())
    }

    #[test]
    fn available_bytes_shrink_after_import() -> Result<()> {
        let temp = tempdir()?;
        let path = temp.path().join("card.ps2");
        create_test_card(&path, false);
        let mut card = MemoryCard::open(&path, true)?;
        let before = card.available_bytes()?;
        let total = card.total_bytes()?;
        assert!(before <= total);
        let cluster_size = u64::from(card.superblock.cluster_size());
        drop(card);

        let files = vec![sample_file("DATA", vec![1, 2, 3])];
        MemoryCard::open(&path, true)?.add_directory("NEW-SAVE", 0x8420, [0; 8], [0; 8], &files)?;

        let mut card = MemoryCard::open(&path, false)?;
        let after = card.available_bytes()?;
        assert_eq!(card.total_bytes()?, total);
        // Save directory needs a second cluster for its third entry, plus
        // one cluster for the file payload and one for root expansion.
        assert_eq!(before - after, 4 * cluster_size);
        Ok(())
    }
}
