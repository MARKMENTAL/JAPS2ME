//! Unicode table rendering for `japs2me list`.

use crate::memory_card::card::SaveSummary;
use unicode_width::UnicodeWidthStr;

const HEADERS: [&str; 4] = ["Save directory", "Game", "Files", "Size"];

/// Format a byte count with binary units.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.0;
    let mut unit = 1;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

fn pad_cell(content: &str, width: usize, right_align: bool) -> String {
    let content_width = UnicodeWidthStr::width(content);
    let padding = width.saturating_sub(content_width);
    if right_align {
        format!("{}{content}", " ".repeat(padding))
    } else {
        format!("{content}{}", " ".repeat(padding))
    }
}

/// Render save summaries as a Unicode table with a free-space footer.
pub fn format_save_table(summaries: &[SaveSummary], free_bytes: u64, total_bytes: u64) -> String {
    let rows: Vec<[String; 4]> = summaries
        .iter()
        .map(|summary| {
            [
                summary.directory.clone(),
                summary.game_title.clone(),
                summary.file_count.to_string(),
                format_size(summary.size_bytes),
            ]
        })
        .collect();

    let mut widths = HEADERS.map(UnicodeWidthStr::width);
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(UnicodeWidthStr::width(cell.as_str()));
        }
    }

    let border = |left: &str, middle: &str, right: &str| -> String {
        let mut line = String::from(left);
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                line.push_str(middle);
            }
            line.push_str(&"─".repeat(width + 2));
        }
        line.push_str(right);
        line
    };
    let row_line = |cells: &[String; 4]| -> String {
        let mut line = String::from("│");
        for (index, cell) in cells.iter().enumerate() {
            let right_align = index >= 2;
            line.push(' ');
            line.push_str(&pad_cell(cell, widths[index], right_align));
            line.push_str(" │");
        }
        line
    };

    let header_cells = HEADERS.map(String::from);
    let mut output = String::new();
    output.push_str(&border("┌", "┬", "┐"));
    output.push('\n');
    output.push_str(&row_line(&header_cells));
    output.push('\n');
    output.push_str(&border("├", "┼", "┤"));
    output.push('\n');
    for row in &rows {
        output.push_str(&row_line(row));
        output.push('\n');
    }
    output.push_str(&border("└", "┴", "┘"));
    output.push('\n');
    output.push_str(&format!(
        "Available space: {} free of {} total",
        format_size(free_bytes),
        format_size(total_bytes)
    ));
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_byte_counts() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(8 * 1024 * 1024), "8.0 MiB");
    }

    #[test]
    fn renders_table_and_footer() {
        let summaries = vec![SaveSummary {
            directory: "BESLES-51912PES3OPT".to_owned(),
            game_title: "P.E.Soccer3 OPTIONFILE".to_owned(),
            file_count: 3,
            size_bytes: 807_484,
        }];
        let table = format_save_table(&summaries, 7_340_032, 8_388_608);
        assert!(table.contains("┌"));
        assert!(table.contains("Save directory"));
        assert!(table.contains("BESLES-51912PES3OPT"));
        assert!(table.contains("P.E.Soccer3 OPTIONFILE"));
        assert!(table.contains("Available space:"));
        assert!(table.contains("free of"));
    }

    #[test]
    fn renders_empty_card() {
        let table = format_save_table(&[], 1024, 2048);
        assert!(table.contains("Save directory"));
        assert!(table.contains("Available space: 1.0 KiB free of 2.0 KiB total"));
    }
}
