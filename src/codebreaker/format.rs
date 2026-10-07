const RC4S: [u8; 256] = [
    0x5f, 0x1f, 0x85, 0x6f, 0x31, 0xaa, 0x3b, 0x18, 0x21, 0xb9, 0xce, 0x1c, 0x07, 0x4c, 0x9c, 0xb4,
    0x81, 0xb8, 0xef, 0x98, 0x59, 0xae, 0xf9, 0x26, 0xe3, 0x80, 0xa3, 0x29, 0x2d, 0x73, 0x51, 0x62,
    0x7c, 0x64, 0x46, 0xf4, 0x34, 0x1a, 0xf6, 0xe1, 0xba, 0x3a, 0x0d, 0x82, 0x79, 0x0a, 0x5c, 0x16,
    0x71, 0x49, 0x8e, 0xac, 0x8c, 0x9f, 0x35, 0x19, 0x45, 0x94, 0x3f, 0x56, 0x0c, 0x91, 0x00, 0x0b,
    0xd7, 0xb0, 0xdd, 0x39, 0x66, 0xa1, 0x76, 0x52, 0x13, 0x57, 0xf3, 0xbb, 0x4e, 0xe5, 0xdc, 0xf0,
    0x65, 0x84, 0xb2, 0xd6, 0xdf, 0x15, 0x3c, 0x63, 0x1d, 0x89, 0x14, 0xbd, 0xd2, 0x36, 0xfe, 0xb1,
    0xca, 0x8b, 0xa4, 0xc6, 0x9e, 0x67, 0x47, 0x37, 0x42, 0x6d, 0x6a, 0x03, 0x92, 0x70, 0x05, 0x7d,
    0x96, 0x2f, 0x40, 0x90, 0xc4, 0xf1, 0x3e, 0x3d, 0x01, 0xf7, 0x68, 0x1e, 0xc3, 0xfc, 0x72, 0xb5,
    0x54, 0xcf, 0xe7, 0x41, 0xe4, 0x4d, 0x83, 0x55, 0x12, 0x22, 0x09, 0x78, 0xfa, 0xde, 0xa7, 0x06,
    0x08, 0x23, 0xbf, 0x0f, 0xcc, 0xc1, 0x97, 0x61, 0xc5, 0x4a, 0xe6, 0xa0, 0x11, 0xc2, 0xea, 0x74,
    0x02, 0x87, 0xd5, 0xd1, 0x9d, 0xb7, 0x7e, 0x38, 0x60, 0x53, 0x95, 0x8d, 0x25, 0x77, 0x10, 0x5e,
    0x9b, 0x7f, 0xd8, 0x6e, 0xda, 0xa2, 0x2e, 0x20, 0x4f, 0xcd, 0x8f, 0xcb, 0xbe, 0x5a, 0xe0, 0xed,
    0x2c, 0x9a, 0xd4, 0xe2, 0xaf, 0xd0, 0xa9, 0xe8, 0xad, 0x7a, 0xbc, 0xa8, 0xf2, 0xee, 0xeb, 0xf5,
    0xa6, 0x99, 0x28, 0x24, 0x6c, 0x2b, 0x75, 0x5d, 0xf8, 0xd3, 0x86, 0x17, 0xfb, 0xc0, 0x7b, 0xb3,
    0x58, 0xdb, 0xc7, 0x4b, 0xff, 0x04, 0x50, 0xe9, 0x88, 0x69, 0xc9, 0x2a, 0xab, 0xfd, 0x5b, 0x1b,
    0x8a, 0xd9, 0xec, 0x27, 0x44, 0x0e, 0x33, 0xc8, 0x6b, 0x93, 0x32, 0x48, 0xb6, 0x30, 0x43, 0xa5,
];

use crate::save::{SaveFile, SaveImport};
use anyhow::{anyhow, bail, ensure, Context, Result};
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Cursor, Read, Write};

const MAGIC: &[u8; 4] = b"CFU\0";
const HEADER_MIN: usize = 124;
const MAX_DECOMPRESSED_BODY: usize = 64 * 1024 * 1024;
const DIRNAME_LEN: usize = 32;
const FILENAME_LEN: usize = 32;

pub type CodebreakerFileEntry = SaveFile;
pub type CodebreakerSave = SaveImport;

fn crypt(mut data: Vec<u8>) -> Vec<u8> {
    let mut state = RC4S;
    let mut j = 0usize;
    for (index, byte) in data.iter_mut().enumerate() {
        let i = (index + 1) & 0xff;
        j = (j + state[i] as usize) & 0xff;
        state.swap(i, j);
        *byte ^= state[(state[i] as usize + state[j] as usize) & 0xff];
    }
    data
}

fn read_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

fn slice<const N: usize>(data: &[u8], offset: usize) -> Result<[u8; N]> {
    data.get(
        offset
            ..offset
                .checked_add(N)
                .ok_or_else(|| anyhow!("offset overflow"))?,
    )
    .ok_or_else(|| anyhow!("truncated Codebreaker header"))?
    .try_into()
    .map_err(|_| anyhow!("truncated Codebreaker header"))
}

pub fn parse(data: &[u8]) -> Result<SaveImport> {
    ensure!(data.len() >= 12, "Codebreaker file is too short");
    ensure!(&data[..4] == MAGIC, "not a Codebreaker save");

    let header_len = u32::from_le_bytes(slice::<4>(data, 8)?) as usize;
    ensure!(header_len >= HEADER_MIN, "Codebreaker header is too short");
    ensure!(header_len <= data.len(), "truncated Codebreaker header");

    let decompressed_len = u32::from_le_bytes(slice::<4>(data, 12)?) as usize;
    let stored_len = u32::from_le_bytes(slice::<4>(data, 16)?) as usize;
    ensure!(
        decompressed_len <= MAX_DECOMPRESSED_BODY,
        "Codebreaker body exceeds size limit"
    );
    ensure!(
        stored_len == data.len() || stored_len == data.len() - header_len,
        "Codebreaker body length does not match header"
    );

    let encrypted_body = crypt(data[header_len..].to_vec());
    let decoder = ZlibDecoder::new(Cursor::new(encrypted_body));
    let mut body = Vec::with_capacity(decompressed_len);
    decoder
        .take((decompressed_len as u64).saturating_add(1))
        .read_to_end(&mut body)
        .context("decompressing Codebreaker save body")?;
    ensure!(
        body.len() == decompressed_len,
        "decompressed Codebreaker body length mismatch"
    );

    let directory = read_string(&data[20..52]);
    let created = slice::<8>(data, 52)?;
    let modified = slice::<8>(data, 60)?;
    let raw_directory_mode = u32::from_le_bytes(slice::<4>(data, 76)?) as u16;
    let directory_mode = if raw_directory_mode & 0x8030 == 0x8020 {
        raw_directory_mode
    } else {
        0x8427 // RWX | directory | 0400 | exists
    };
    let title = read_string(&data[92..header_len]);

    let mut files = Vec::new();
    let mut offset = 0usize;
    while offset < body.len() {
        ensure!(
            body.len() - offset >= 64,
            "truncated Codebreaker file header"
        );
        let created = slice::<8>(&body, offset)?;
        let modified = slice::<8>(&body, offset + 8)?;
        let size = u32::from_le_bytes(slice::<4>(&body, offset + 16)?) as usize;
        let mode = u16::from_le_bytes(slice::<2>(&body, offset + 20)?);
        let name = read_string(&body[offset + 32..offset + 64]);
        ensure!(
            mode & 0x8030 == 0x8010,
            "Codebreaker entry is not a regular file: {name}"
        );

        let data_start = offset
            .checked_add(64)
            .ok_or_else(|| anyhow!("file offset overflow"))?;
        let data_end = data_start
            .checked_add(size)
            .ok_or_else(|| anyhow!("file size overflow"))?;
        let file_data = body
            .get(data_start..data_end)
            .ok_or_else(|| anyhow!("truncated Codebreaker file data: {name}"))?;
        files.push(SaveFile {
            name,
            data: file_data.to_vec(),
            mode,
            created,
            modified,
        });
        offset = data_end;
    }

    if directory.is_empty() {
        bail!("Codebreaker save has an empty directory name");
    }
    Ok(SaveImport {
        directory,
        directory_mode,
        created,
        modified,
        title,
        files,
    })
}

fn validate_entry_name(name: &str, what: &str) -> Result<()> {
    ensure!(!name.is_empty(), "Codebreaker {what} name cannot be empty");
    ensure!(
        name.len() <= DIRNAME_LEN,
        "Codebreaker {what} name must fit in 32 bytes: {name}"
    );
    ensure!(
        !name.bytes().any(|byte| byte == 0),
        "Codebreaker {what} name cannot contain NUL: {name}"
    );
    Ok(())
}

/// Encode a save as a Codebreaker `.cbs` file.
///
/// Mirrors [`parse`]: a `CFU\0` header with lengths, directory metadata,
/// and title, followed by the RC4-encrypted zlib body of 64-byte file
/// records plus raw file data.
pub fn encode(save: &SaveImport) -> Result<Vec<u8>> {
    validate_entry_name(&save.directory, "directory")?;
    ensure!(
        !save.title.bytes().any(|byte| byte == 0),
        "Codebreaker title cannot contain NUL"
    );
    for file in &save.files {
        validate_entry_name(&file.name, "file")?;
        ensure!(
            file.mode & 0x8030 == 0x8010,
            "Codebreaker entry is not a regular file: {}",
            file.name
        );
        u32::try_from(file.data.len()).context("file is too large for Codebreaker save")?;
    }

    let mut body = Vec::new();
    for file in &save.files {
        body.extend_from_slice(&file.created);
        body.extend_from_slice(&file.modified);
        body.extend_from_slice(
            &u32::try_from(file.data.len())
                .context("file is too large for Codebreaker save")?
                .to_le_bytes(),
        );
        body.extend_from_slice(&file.mode.to_le_bytes());
        body.extend_from_slice(&[0u8; 2]); // reserved
        body.extend_from_slice(&[0u8; 8]); // reserved
        let mut name_field = [0u8; FILENAME_LEN];
        name_field[..file.name.len()].copy_from_slice(file.name.as_bytes());
        body.extend_from_slice(&name_field);
        body.extend_from_slice(&file.data);
    }
    ensure!(
        body.len() <= MAX_DECOMPRESSED_BODY,
        "Codebreaker body exceeds size limit"
    );

    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&body)
        .context("compressing Codebreaker save body")?;
    let encrypted = crypt(
        encoder
            .finish()
            .context("compressing Codebreaker save body")?,
    );

    let title_bytes = save.title.as_bytes();
    let header_len = HEADER_MIN.max(
        92usize
            .checked_add(title_bytes.len())
            .and_then(|len| len.checked_add(1))
            .ok_or_else(|| anyhow!("Codebreaker title is too long"))?,
    );
    let stored_len = header_len
        .checked_add(encrypted.len())
        .ok_or_else(|| anyhow!("Codebreaker save is too large"))?;
    let header_len_u32 = u32::try_from(header_len).context("Codebreaker header is too large")?;
    let stored_len_u32 = u32::try_from(stored_len).context("Codebreaker save is too large")?;
    let body_len_u32 = u32::try_from(body.len()).context("Codebreaker body is too large")?;

    let directory_mode = if save.directory_mode & 0x8030 == 0x8020 {
        u32::from(save.directory_mode)
    } else {
        0x8427 // RWX | directory | 0400 | exists
    };

    let mut header = vec![0u8; header_len];
    header[0..4].copy_from_slice(MAGIC);
    header[8..12].copy_from_slice(&header_len_u32.to_le_bytes());
    header[12..16].copy_from_slice(&body_len_u32.to_le_bytes());
    header[16..20].copy_from_slice(&stored_len_u32.to_le_bytes());
    header[20..20 + save.directory.len()].copy_from_slice(save.directory.as_bytes());
    header[52..60].copy_from_slice(&save.created);
    header[60..68].copy_from_slice(&save.modified);
    header[76..80].copy_from_slice(&directory_mode.to_le_bytes());
    header[92..92 + title_bytes.len()].copy_from_slice(title_bytes);

    header.extend_from_slice(&encrypted);
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::{crypt, encode, parse};
    use crate::save::{SaveFile, SaveImport};
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    fn sample_cbs() -> Vec<u8> {
        let mut body = vec![0u8; 64];
        body[0..8].copy_from_slice(&[0, 1, 2, 3, 4, 5, 0xe8, 0x07]);
        body[8..16].copy_from_slice(&[0, 6, 7, 8, 9, 5, 0xe8, 0x07]);
        body[16..20].copy_from_slice(&3u32.to_le_bytes());
        body[20..22].copy_from_slice(&0x8017u16.to_le_bytes());
        body[32..40].copy_from_slice(b"DATA.BIN");
        body.extend_from_slice(&[0xaa, 0xbb, 0xcc]);

        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&body).unwrap();
        let compressed = crypt(encoder.finish().unwrap());

        let header_len = 124usize;
        let mut cbs = vec![0u8; header_len];
        cbs[0..4].copy_from_slice(b"CFU\0");
        cbs[8..12].copy_from_slice(&(header_len as u32).to_le_bytes());
        cbs[12..16].copy_from_slice(&(body.len() as u32).to_le_bytes());
        cbs[16..20].copy_from_slice(&((header_len + compressed.len()) as u32).to_le_bytes());
        cbs[20..31].copy_from_slice(b"BASLUS-TEST");
        cbs[52..60].copy_from_slice(&[0, 1, 2, 3, 4, 5, 0xe8, 0x07]);
        cbs[60..68].copy_from_slice(&[0, 6, 7, 8, 9, 5, 0xe8, 0x07]);
        cbs[76..80].copy_from_slice(&0x8420u32.to_le_bytes());
        cbs[92..101].copy_from_slice(b"Test Save");
        cbs.extend_from_slice(&compressed);
        cbs
    }

    #[test]
    fn parses_header_metadata_and_file_record() {
        let save = parse(&sample_cbs()).unwrap();
        assert_eq!(save.directory, "BASLUS-TEST");
        assert_eq!(save.title, "Test Save");
        assert_eq!(save.directory_mode, 0x8420);
        assert_eq!(save.files.len(), 1);
        assert_eq!(save.files[0].name, "DATA.BIN");
        assert_eq!(save.files[0].data, [0xaa, 0xbb, 0xcc]);
        assert_eq!(save.files[0].mode, 0x8017);
    }

    #[test]
    fn rejects_truncated_or_inconsistent_cbs() {
        assert!(parse(b"CFU\0").is_err());
        let mut cbs = sample_cbs();
        cbs.truncate(100);
        assert!(parse(&cbs).is_err());
    }

    fn sample_save() -> SaveImport {
        SaveImport {
            directory: "BASLUS-TEST".to_owned(),
            directory_mode: 0x8420,
            created: [0, 1, 2, 3, 4, 5, 0xe8, 0x07],
            modified: [0, 6, 7, 8, 9, 5, 0xe8, 0x07],
            title: "Test Save".to_owned(),
            files: vec![
                SaveFile {
                    name: "DATA.BIN".to_owned(),
                    data: vec![0xaa, 0xbb, 0xcc],
                    mode: 0x8017,
                    created: [0, 1, 2, 3, 4, 5, 0xe8, 0x07],
                    modified: [0, 6, 7, 8, 9, 5, 0xe8, 0x07],
                },
                SaveFile {
                    name: "EMPTY".to_owned(),
                    data: vec![],
                    mode: 0x8417,
                    created: [0; 8],
                    modified: [0; 8],
                },
            ],
        }
    }

    fn assert_same_save(expected: &SaveImport, actual: &SaveImport) {
        assert_eq!(actual.directory, expected.directory);
        assert_eq!(actual.directory_mode, expected.directory_mode);
        assert_eq!(actual.created, expected.created);
        assert_eq!(actual.modified, expected.modified);
        assert_eq!(actual.title, expected.title);
        assert_eq!(actual.files.len(), expected.files.len());
        for (actual, expected) in actual.files.iter().zip(expected.files.iter()) {
            assert_eq!(actual.name, expected.name);
            assert_eq!(actual.data, expected.data);
            assert_eq!(actual.mode, expected.mode);
            assert_eq!(actual.created, expected.created);
            assert_eq!(actual.modified, expected.modified);
        }
    }

    #[test]
    fn encode_round_trips_through_parse() {
        let save = sample_save();
        let encoded = encode(&save).unwrap();
        let parsed = parse(&encoded).unwrap();
        assert_same_save(&save, &parsed);
    }

    #[test]
    fn parsed_save_reencodes_to_equivalent_save() {
        let parsed = parse(&sample_cbs()).unwrap();
        let reencoded = encode(&parsed).unwrap();
        let reparsed = parse(&reencoded).unwrap();
        assert_same_save(&parsed, &reparsed);
    }

    #[test]
    fn encode_grows_header_for_long_titles() {
        let mut save = sample_save();
        save.title = "A".repeat(200);
        let encoded = encode(&save).unwrap();
        let header_len = u32::from_le_bytes(encoded[8..12].try_into().unwrap()) as usize;
        assert_eq!(header_len, 92 + 200 + 1);
        assert_eq!(parse(&encoded).unwrap().title, save.title);
    }

    #[test]
    fn encode_rejects_invalid_names_and_modes() {
        let mut save = sample_save();
        save.directory.clear();
        assert!(encode(&save).is_err());

        let mut save = sample_save();
        save.directory = "X".repeat(33);
        assert!(encode(&save).is_err());

        let mut save = sample_save();
        save.files[0].name = "not-a-file\0name".to_owned();
        assert!(encode(&save).is_err());

        let mut save = sample_save();
        save.files[0].mode = 0x8427; // directory mode, not a regular file
        assert!(encode(&save).is_err());
    }
}
