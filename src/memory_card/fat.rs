//! FAT constants and chain helpers.

pub const ALLOCATED: u32 = 0x8000_0000;
pub const CHAIN_END: u32 = 0xffff_ffff;
pub const CHAIN_END_UNALLOC: u32 = 0x7fff_ffff;

pub fn next_cluster(value: u32) -> Option<u32> {
    if value == CHAIN_END {
        None
    } else if value & ALLOCATED != 0 {
        Some(value & !ALLOCATED)
    } else {
        None
    }
}
