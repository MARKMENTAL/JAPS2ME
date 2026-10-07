//! Parsing for PS2 `icon.sys` title metadata.
//!
//! The layout follows the `icon.sys` structure used by mymcplusplus:
//! 4-byte `PS2D` magic, title split offset at byte 6, and a 68-byte
//! Shift-JIS title field at byte 192.

const ICON_SYS_LEN: usize = 964;
const ICON_SYS_MAGIC: &[u8] = b"PS2D";
const TITLE_OFFSET: usize = 192;
const TITLE_LEN: usize = 68;
const TITLE_SPLIT_OFFSET: usize = 6;
const MAX_TITLE_SPLIT: usize = TITLE_LEN;

/// Extract the display title from raw `icon.sys` bytes.
///
/// Returns `None` when the data is not a valid `icon.sys` file or contains
/// no usable title text. The two title lines are joined with whitespace
/// collapsed, matching mymcplusplus `single_title` behavior.
pub fn parse_title(data: &[u8]) -> Option<String> {
    if data.len() != ICON_SYS_LEN {
        return None;
    }
    if &data[0..ICON_SYS_MAGIC.len()] != ICON_SYS_MAGIC {
        return None;
    }
    let split =
        u16::from_le_bytes([data[TITLE_SPLIT_OFFSET], data[TITLE_SPLIT_OFFSET + 1]]) as usize;
    if split > MAX_TITLE_SPLIT {
        return None;
    }

    let title_field = &data[TITLE_OFFSET..TITLE_OFFSET + TITLE_LEN];
    let end = title_field
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(title_field.len());
    let title_field = &title_field[..end];
    let split = split.min(title_field.len());
    let (first, second) = title_field.split_at(split);

    let first = decode_shift_jis(first);
    let second = decode_shift_jis(second);
    let combined = format!("{first} {second}");
    let title = combined.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        None
    } else {
        Some(title)
    }
}

fn decode_shift_jis(bytes: &[u8]) -> String {
    // `encoding_rs` replaces malformed sequences, matching the Python
    // `decode("shift_jis", "replace")` behavior closely enough for titles.
    encoding_rs::SHIFT_JIS.decode(bytes).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_icon_sys(title: &[u8], split: u16) -> Vec<u8> {
        let mut data = vec![0u8; ICON_SYS_LEN];
        data[0..4].copy_from_slice(b"PS2D");
        data[TITLE_SPLIT_OFFSET..TITLE_SPLIT_OFFSET + 2].copy_from_slice(&split.to_le_bytes());
        let len = title.len().min(TITLE_LEN);
        data[TITLE_OFFSET..TITLE_OFFSET + len].copy_from_slice(&title[..len]);
        data
    }

    #[test]
    fn parses_two_line_ascii_title() {
        let mut title = vec![b' '; TITLE_LEN];
        let first = b"My Game";
        let second = b"Bonus";
        title[..first.len()].copy_from_slice(first);
        title[16..16 + second.len()].copy_from_slice(second);
        let data = sample_icon_sys(&title, 16);
        assert_eq!(parse_title(&data).as_deref(), Some("My Game Bonus"));
    }

    #[test]
    fn rejects_bad_magic_and_length() {
        assert_eq!(parse_title(&[0u8; 100]), None);
        let mut data = sample_icon_sys(b"Title", 5);
        data[0] = b'X';
        assert_eq!(parse_title(&data), None);
    }

    #[test]
    fn rejects_out_of_range_split() {
        let data = sample_icon_sys(b"Title", 99);
        assert_eq!(parse_title(&data), None);
    }

    #[test]
    fn returns_none_for_empty_title() {
        let data = sample_icon_sys(&[0u8; TITLE_LEN], 0);
        assert_eq!(parse_title(&data), None);
    }

    #[test]
    fn decodes_shift_jis_title() {
        // "ゲーム" in Shift-JIS.
        let shift_jis = [0x83, 0x51, 0x81, 0x5b, 0x83, 0x80];
        let data = sample_icon_sys(&shift_jis, 0);
        assert_eq!(parse_title(&data).as_deref(), Some("ゲーム"));
    }
}
