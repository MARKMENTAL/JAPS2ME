//! MAX Drive (`.max`) save parsing, including its LZARI-compressed payload.

use crate::save::{SaveFile, SaveImport};
use anyhow::{anyhow, ensure, Context, Result};
use crc32fast::Hasher;

const MAGIC: &[u8; 12] = b"Ps2PowerSave";
const HEADER_LEN: usize = 0x5c;
const MAX_DECOMPRESSED_LEN: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 4096;

const HISTORY_LEN: usize = 4096;
const MIN_MATCH_LEN: usize = 3;
const MAX_MATCH_LEN: usize = 60;
const MAX_CHAR: usize = 256 + MAX_MATCH_LEN - MIN_MATCH_LEN + 1;
const ARITH_BITS: u32 = 15;
const Q1: u32 = 1 << ARITH_BITS;
const Q2: u32 = Q1 * 2;
const Q3: u32 = Q1 * 3;
const Q4: u32 = Q1 * 4;
const MAX_CUM: u32 = Q1 - 1;

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(
            offset
                ..offset
                    .checked_add(4)
                    .ok_or_else(|| anyhow!("offset overflow"))?,
        )
        .ok_or_else(|| anyhow!("truncated MAX Drive header"))?;
    Ok(u32::from_le_bytes(bytes.try_into()?))
}

fn read_name(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn check_crc(header: &[u8], compressed: &[u8], expected: u32) -> Result<()> {
    let mut hasher = Hasher::new();
    hasher.update(&header[..12]);
    hasher.update(&[0; 4]);
    hasher.update(&header[16..]);
    hasher.update(compressed);
    ensure!(
        hasher.finalize() == expected,
        "MAX Drive CRC mismatch (save may be damaged)"
    );
    Ok(())
}

pub fn parse(data: &[u8]) -> Result<SaveImport> {
    ensure!(
        data.len() >= HEADER_LEN,
        "MAX Drive save is shorter than its header"
    );
    ensure!(&data[..12] == MAGIC, "not a MAX Drive save");

    let expected_crc = read_u32(data, 12)?;
    let directory = read_name(&data[16..48]);
    let icon_title = read_name(&data[48..80]);
    let compressed_len = read_u32(data, 80)? as usize;
    let file_count = read_u32(data, 84)? as usize;
    let decompressed_len = read_u32(data, 88)? as usize;
    ensure!(
        !directory.is_empty(),
        "MAX Drive save has an empty directory name"
    );
    ensure!(
        file_count <= MAX_FILES,
        "MAX Drive save contains too many files"
    );
    ensure!(
        decompressed_len <= MAX_DECOMPRESSED_LEN,
        "MAX Drive decompressed body exceeds the size limit"
    );

    // Most files store compressed size + four bytes here. Some MAX files
    // instead store the uncompressed size, in which case the payload runs to
    // EOF; this convention is also used by mymcplusplus.
    let payload_len = if compressed_len == decompressed_len {
        data.len() - HEADER_LEN
    } else {
        compressed_len
            .checked_sub(4)
            .ok_or_else(|| anyhow!("invalid MAX Drive compressed length"))?
    };
    let payload_end = HEADER_LEN
        .checked_add(payload_len)
        .ok_or_else(|| anyhow!("MAX Drive payload length overflow"))?;
    let compressed = data
        .get(HEADER_LEN..payload_end)
        .ok_or_else(|| anyhow!("truncated MAX Drive compressed payload"))?;
    check_crc(&data[..HEADER_LEN], compressed, expected_crc)?;

    let body =
        lzari::decode(compressed, decompressed_len).context("decompressing MAX Drive save")?;
    let mut files = Vec::with_capacity(file_count);
    let mut offset = 0usize;
    for _ in 0..file_count {
        let record_header_end = offset
            .checked_add(36)
            .ok_or_else(|| anyhow!("MAX Drive file-record offset overflow"))?;
        let record_header = body
            .get(offset..record_header_end)
            .ok_or_else(|| anyhow!("truncated MAX Drive file record"))?;
        let file_len = u32::from_le_bytes(record_header[..4].try_into()?) as usize;
        let name = read_name(&record_header[4..36]);
        let file_end = record_header_end
            .checked_add(file_len)
            .ok_or_else(|| anyhow!("MAX Drive file size overflow"))?;
        let file_data = body
            .get(record_header_end..file_end)
            .ok_or_else(|| anyhow!("truncated MAX Drive file data: {name}"))?;
        files.push(SaveFile {
            name,
            data: file_data.to_vec(),
            mode: 0x8417, // RWX | file | 0400 | exists
            created: [0; 8],
            modified: [0; 8],
        });
        // MAX records are padded according to round_up(offset + 8, 16) - 8.
        offset = file_end
            .checked_add(23)
            .ok_or_else(|| anyhow!("MAX Drive alignment overflow"))?
            / 16
            * 16
            - 8;
        ensure!(offset <= body.len(), "invalid MAX Drive record padding");
    }
    ensure!(
        offset == body.len(),
        "MAX Drive body has unexpected trailing data"
    );

    Ok(SaveImport {
        directory,
        directory_mode: 0x8427, // RWX | directory | 0400 | exists
        created: [0; 8],
        modified: [0; 8],
        title: icon_title,
        files,
    })
}

mod lzari {
    use super::{
        anyhow, ensure, Result, ARITH_BITS, HISTORY_LEN, MAX_CHAR, MAX_CUM, MAX_MATCH_LEN,
        MIN_MATCH_LEN, Q1, Q2, Q3, Q4,
    };

    struct BitReader<'a> {
        source: &'a [u8],
        bit_position: usize,
    }

    impl<'a> BitReader<'a> {
        fn new(source: &'a [u8]) -> Self {
            Self {
                source,
                bit_position: 0,
            }
        }

        fn next(&mut self) -> Result<u32> {
            let byte_index = self.bit_position / 8;
            let byte = *self
                .source
                .get(byte_index)
                .ok_or_else(|| anyhow!("truncated LZARI bitstream"))?;
            let bit = u32::from((byte >> (7 - self.bit_position % 8)) & 1);
            self.bit_position += 1;
            Ok(bit)
        }
    }

    struct Decoder<'a> {
        bits: BitReader<'a>,
        low: u32,
        high: u32,
        code: u32,
        frequencies: Vec<u32>,
        cumulative: Vec<u32>,
        symbol_to_char: Vec<usize>,
        position_cumulative: Vec<u32>,
    }

    impl<'a> Decoder<'a> {
        fn new(source: &'a [u8]) -> Result<Self> {
            let mut bits = BitReader::new(source);
            let mut code = 0;
            for _ in 0..ARITH_BITS + 2 {
                code = code * 2 + bits.next()?;
            }

            let mut position_cumulative = vec![0; HISTORY_LEN + 1];
            let mut cumulative = 0;
            for i in (1..=HISTORY_LEN).rev() {
                cumulative += 10000 / (200 + i as u32);
                position_cumulative[i - 1] = cumulative;
            }

            Ok(Self {
                bits,
                low: 0,
                high: Q4,
                code,
                frequencies: (0..=MAX_CHAR).map(|i| u32::from(i != 0)).collect(),
                cumulative: (0..=MAX_CHAR as u32).collect(),
                symbol_to_char: (0..=MAX_CHAR).map(|i| i.saturating_sub(1)).collect(),
                position_cumulative,
            })
        }

        fn update_model(&mut self, symbol: usize) {
            if self.cumulative[MAX_CHAR] >= MAX_CUM {
                let mut sum = 0;
                for i in (1..=MAX_CHAR).rev() {
                    self.cumulative[MAX_CHAR - i] = sum;
                    self.frequencies[i] = self.frequencies[i].div_ceil(2);
                    sum += self.frequencies[i];
                }
                self.cumulative[MAX_CHAR] = sum;
            }

            let frequency = self.frequencies[symbol];
            let mut new_symbol = symbol;
            while self.frequencies[new_symbol - 1] == frequency {
                new_symbol -= 1;
            }
            if new_symbol != symbol {
                self.symbol_to_char.swap(new_symbol, symbol);
            }
            self.frequencies[new_symbol] += 1;
            for value in &mut self.cumulative[MAX_CHAR - new_symbol + 1..=MAX_CHAR] {
                *value += 1;
            }
        }

        fn normalize_char(&mut self) -> Result<()> {
            loop {
                if self.low < Q2 {
                    if self.low < Q1 || self.high > Q3 {
                        if self.high > Q2 {
                            break;
                        }
                    } else {
                        self.low -= Q1;
                        self.code -= Q1;
                        self.high -= Q1;
                    }
                } else {
                    self.low -= Q2;
                    self.code -= Q2;
                    self.high -= Q2;
                }
                self.low *= 2;
                self.high *= 2;
                self.code = self.code * 2 + self.bits.next()?;
            }
            Ok(())
        }

        fn decode_char(&mut self) -> Result<usize> {
            let range = self.high - self.low;
            let max_frequency = self.cumulative[MAX_CHAR];
            let value = ((self.code - self.low + 1) * max_frequency - 1) / range;
            let i = self.cumulative[1..].partition_point(|&frequency| frequency <= value) + 1;
            self.high = self.low + self.cumulative[i] * range / max_frequency;
            self.low += self.cumulative[i - 1] * range / max_frequency;
            let symbol = MAX_CHAR + 1 - i;
            self.normalize_char()?;
            let character = self.symbol_to_char[symbol];
            self.update_model(symbol);
            Ok(character)
        }

        fn decode_position(&mut self) -> Result<usize> {
            let range = self.high - self.low;
            let max_cumulative = self.position_cumulative[0];
            let value = ((self.code - self.low + 1) * max_cumulative - 1) / range;

            // The reference uses a binary search for the first cumulative
            // entry less than or equal to `value` in this decreasing table.
            let mut c = 1usize;
            let mut s = self.position_cumulative.len() - 1;
            loop {
                let middle = (s + c) / 2;
                if self.position_cumulative[middle] <= value {
                    s = middle;
                } else {
                    c = middle + 1;
                }
                if c >= s {
                    break;
                }
            }
            let position = c - 1;
            self.high = self.low + self.position_cumulative[position] * range / max_cumulative;
            self.low += self.position_cumulative[position + 1] * range / max_cumulative;

            loop {
                if self.low < Q2 {
                    if self.low < Q1 || self.high > Q3 {
                        if self.high > Q2 {
                            return Ok(position);
                        }
                    } else {
                        self.low -= Q1;
                        self.code -= Q1;
                        self.high -= Q1;
                    }
                } else {
                    self.low -= Q2;
                    self.code -= Q2;
                    self.high -= Q2;
                }
                self.low *= 2;
                self.high *= 2;
                self.code = self.code * 2 + self.bits.next()?;
            }
        }
    }

    pub(super) fn decode(source: &[u8], output_len: usize) -> Result<Vec<u8>> {
        if output_len == 0 {
            return Ok(Vec::new());
        }
        let mut padded = Vec::with_capacity(source.len() + 4);
        padded.extend_from_slice(source);
        padded.extend_from_slice(&[0; 4]); // LZARI specifies 32 trailing zero bits.
        let mut decoder = Decoder::new(&padded)?;
        let mut output = Vec::with_capacity(output_len);
        let mut history = vec![0x20u8; HISTORY_LEN - MAX_MATCH_LEN];
        history.resize(HISTORY_LEN, 0);
        let mut history_position = HISTORY_LEN - MAX_MATCH_LEN;

        while output.len() < output_len {
            let character = decoder.decode_char()?;
            if character >= 0x100 {
                let length = character - 0x100 + MIN_MATCH_LEN;
                ensure!(
                    output.len() + length <= output_len,
                    "LZARI match exceeds declared output size"
                );
                let position = decoder.decode_position()?;
                let base = (history_position + HISTORY_LEN - position - 1) % HISTORY_LEN;
                for offset in 0..length {
                    let byte = history[(base + offset) % HISTORY_LEN];
                    output.push(byte);
                    history[history_position] = byte;
                    history_position = (history_position + 1) % HISTORY_LEN;
                }
            } else {
                let byte = u8::try_from(character).map_err(|_| anyhow!("invalid LZARI literal"))?;
                output.push(byte);
                history[history_position] = byte;
                history_position = (history_position + 1) % HISTORY_LEN;
            }
        }
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, MAGIC};

    const PES3_MAX: &[u8] = include_bytes!("../save-games/PES3allunlocked.max");

    #[test]
    fn parses_pes3_max_save_and_decodes_files() {
        let save = parse(PES3_MAX).unwrap();
        assert_eq!(save.directory, "BESLES-51912PES3OPT");
        assert_eq!(save.title, "P.E.Soccer3 OPTIONFILE");
        assert_eq!(save.directory_mode, 0x8427);
        assert_eq!(save.files.len(), 3);

        let expected = [
            ("icon.sys", 964usize, 0x949c_9e42u32),
            ("icon_view", 33_400usize, 0x1fa1_4a00u32),
            ("BESLES-51912PES3OPT", 773_120usize, 0x3f2a_99afu32),
        ];
        for (file, (name, size, crc)) in save.files.iter().zip(expected) {
            assert_eq!(file.name, name);
            assert_eq!(file.data.len(), size);
            assert_eq!(crc32fast::hash(&file.data), crc);
            assert_eq!(file.mode, 0x8417);
        }
    }

    #[test]
    fn rejects_bad_max_magic_and_crc() {
        assert!(parse(b"not a max save").is_err());
        let mut damaged = PES3_MAX.to_vec();
        damaged[12] ^= 1;
        assert!(parse(&damaged).is_err());
        assert_eq!(&PES3_MAX[..12], MAGIC);
    }
}
