//! Hamming ECC used in the spare area of PS2 memory-card pages.

fn parity(value: u8) -> u8 {
    (value.count_ones() & 1) as u8
}

fn ecc_128(data: &[u8]) -> [u8; 3] {
    let masks = [0x55, 0x33, 0x0f, 0x00, 0xaa, 0xcc, 0xf0];
    let mut column = 0x77u8;
    let mut line0 = 0x7fu8;
    let mut line1 = 0x7fu8;

    for (index, &byte) in data.iter().enumerate() {
        for (bit, mask) in masks.iter().enumerate() {
            column ^= parity(byte & mask) << bit;
        }
        if parity(byte) != 0 {
            line0 ^= !(index as u8);
            line1 ^= index as u8;
        }
    }

    [column, line0 & 0x7f, line1]
}

/// Calculate the page spare bytes (three ECC bytes per 128 data bytes,
/// followed by zeroed unused spare bytes).
pub fn page_spare(page: &[u8], spare_len: usize) -> Vec<u8> {
    let mut spare = vec![0; spare_len];
    for (chunk_index, chunk) in page.chunks(128).enumerate() {
        let ecc = ecc_128(chunk);
        let start = chunk_index * 3;
        if start + 3 <= spare.len() {
            spare[start..start + 3].copy_from_slice(&ecc);
        }
    }
    spare
}

#[cfg(test)]
mod tests {
    use super::page_spare;

    #[test]
    fn matches_reference_ecc_for_zero_page() {
        assert_eq!(
            page_spare(&[0; 512], 16),
            [0x77, 0x7f, 0x7f, 0x77, 0x7f, 0x7f, 0x77, 0x7f, 0x7f, 0x77, 0x7f, 0x7f, 0, 0, 0, 0]
        );
    }
}
