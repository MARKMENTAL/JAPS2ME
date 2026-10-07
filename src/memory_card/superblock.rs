//! Superblock parsing and writing for PS2 memory cards.

use anyhow::{anyhow, Result};

/// Magic bytes at the start of a PS2 memory card image.
pub const PS2MC_MAGIC: &[u8] = b"Sony PS2 Memory Card Format ";

/// Standard page size for PS2 memory cards.
pub const PS2MC_STANDARD_PAGE_SIZE: u32 = 512;

/// Standard pages per card (8MB card = 16384 pages).
pub const PS2MC_STANDARD_PAGES_PER_CARD: u32 = 16384;

/// Standard pages per erase block.
pub const PS2MC_STANDARD_PAGES_PER_ERASE_BLOCK: u32 = 16;

/// Cluster size (2 pages of 512 bytes).
pub const PS2MC_CLUSTER_SIZE: u32 = 1024;

/// Offset of the indirect FAT in the card.
pub const PS2MC_INDIRECT_FAT_OFFSET: u32 = 0x2000;

/// Maximum number of indirect FAT clusters.
pub const PS2MC_MAX_INDIRECT_FAT_CLUSTERS: usize = 32;

/// The superblock is the first 512 bytes of the memory card.
#[derive(Debug, Clone)]
pub struct Superblock {
    /// Magic bytes: "Sony PS2 Memory Card Format "
    pub magic: [u8; 28],
    /// Version string, e.g. "1.2.0.0"
    pub version: [u8; 12],
    /// Page size in bytes (typically 512).
    pub page_size: u16,
    /// Pages per cluster (typically 2).
    pub pages_per_cluster: u16,
    /// Pages per erase block (typically 16).
    pages_per_erase_block: u16,
    /// Total clusters per card.
    pub clusters_per_card: u32,
    /// First allocatable cluster.
    pub allocatable_cluster_offset: u32,
    /// Last allocatable cluster + 1.
    pub allocatable_cluster_end: u32,
    /// Root directory FAT cluster (always 0).
    pub rootdir_fat_cluster: u32,
    /// Good block 1 (for ECC).
    pub good_block1: u32,
    /// Good block 2 (for ECC).
    pub good_block2: u32,
    /// Indirect FAT cluster list (up to 32 entries).
    pub indirect_fat_cluster_list: [u32; PS2MC_MAX_INDIRECT_FAT_CLUSTERS],
    /// Bad erase block list (up to 32 entries).
    pub bad_erase_block_list: [u32; PS2MC_MAX_INDIRECT_FAT_CLUSTERS],
}

impl Superblock {
    /// Parse a superblock from raw bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 512 {
            return Err(anyhow!("Superblock data too short: {} bytes", data.len()));
        }
        if !data.starts_with(PS2MC_MAGIC) {
            return Err(anyhow!("Not a PS2 memory card image (bad magic)"));
        }

        let mut magic = [0u8; 28];
        magic.copy_from_slice(&data[0..28]);

        let mut version = [0u8; 12];
        version.copy_from_slice(&data[28..40]);

        let page_size = u16::from_le_bytes([data[40], data[41]]);
        let pages_per_cluster = u16::from_le_bytes([data[42], data[43]]);
        let pages_per_erase_block = u16::from_le_bytes([data[44], data[45]]);

        // Skip 2 bytes padding at offset 46-47
        let clusters_per_card = u32::from_le_bytes([data[48], data[49], data[50], data[51]]);
        let allocatable_cluster_offset =
            u32::from_le_bytes([data[52], data[53], data[54], data[55]]);
        let allocatable_cluster_end = u32::from_le_bytes([data[56], data[57], data[58], data[59]]);
        let rootdir_fat_cluster = u32::from_le_bytes([data[60], data[61], data[62], data[63]]);
        let good_block1 = u32::from_le_bytes([data[64], data[65], data[66], data[67]]);
        let good_block2 = u32::from_le_bytes([data[68], data[69], data[70], data[71]]);

        // Skip 8 bytes padding at offset 72-79
        let mut indirect_fat_cluster_list = [0u32; PS2MC_MAX_INDIRECT_FAT_CLUSTERS];
        for (i, cluster) in indirect_fat_cluster_list.iter_mut().enumerate() {
            let offset = 80 + i * 4;
            *cluster = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
        }

        let mut bad_erase_block_list = [0u32; PS2MC_MAX_INDIRECT_FAT_CLUSTERS];
        for (i, block) in bad_erase_block_list.iter_mut().enumerate() {
            let offset = 208 + i * 4;
            *block = u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
        }

        Ok(Superblock {
            magic,
            version,
            page_size,
            pages_per_cluster,
            pages_per_erase_block,
            clusters_per_card,
            allocatable_cluster_offset,
            allocatable_cluster_end,
            rootdir_fat_cluster,
            good_block1,
            good_block2,
            indirect_fat_cluster_list,
            bad_erase_block_list,
        })
    }

    /// Serialize the superblock to raw bytes (512 bytes).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = vec![0u8; 512];

        buf[0..28].copy_from_slice(&self.magic);
        buf[28..40].copy_from_slice(&self.version);
        buf[40..42].copy_from_slice(&self.page_size.to_le_bytes());
        buf[42..44].copy_from_slice(&self.pages_per_cluster.to_le_bytes());
        buf[44..46].copy_from_slice(&self.pages_per_erase_block.to_le_bytes());
        // padding at 46-47
        buf[48..52].copy_from_slice(&self.clusters_per_card.to_le_bytes());
        buf[52..56].copy_from_slice(&self.allocatable_cluster_offset.to_le_bytes());
        buf[56..60].copy_from_slice(&self.allocatable_cluster_end.to_le_bytes());
        buf[60..64].copy_from_slice(&self.rootdir_fat_cluster.to_le_bytes());
        buf[64..68].copy_from_slice(&self.good_block1.to_le_bytes());
        buf[68..72].copy_from_slice(&self.good_block2.to_le_bytes());
        // padding at 72-79

        for i in 0..PS2MC_MAX_INDIRECT_FAT_CLUSTERS {
            let offset = 80 + i * 4;
            buf[offset..offset + 4]
                .copy_from_slice(&self.indirect_fat_cluster_list[i].to_le_bytes());
        }

        for i in 0..PS2MC_MAX_INDIRECT_FAT_CLUSTERS {
            let offset = 208 + i * 4;
            buf[offset..offset + 4].copy_from_slice(&self.bad_erase_block_list[i].to_le_bytes());
        }

        buf
    }

    /// Get the cluster size in bytes.
    pub fn cluster_size(&self) -> u32 {
        self.page_size as u32 * self.pages_per_cluster as u32
    }

    /// Get the number of entries per FAT cluster.
    pub fn entries_per_cluster(&self) -> u32 {
        self.cluster_size() / 4
    }

    /// Get the spare size (ECC data per page).
    pub fn spare_size(&self) -> u32 {
        (self.page_size as u32).div_ceil(128) * 4
    }

    /// Get the raw page size (page + spare).
    pub fn raw_page_size(&self) -> u32 {
        self.page_size as u32 + self.spare_size()
    }

    /// Get the allocatable cluster limit.
    pub fn allocatable_cluster_limit(&self) -> u32 {
        (self.good_block1.min(self.good_block2) * self.pages_per_erase_block as u32)
            / self.pages_per_cluster as u32
            - self.allocatable_cluster_offset
    }
}
