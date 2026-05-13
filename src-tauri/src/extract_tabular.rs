//! CSV, TSV, JSONL, and Excel → plain text for ingest.

use anyhow::{Context, Result};
use calamine::{open_workbook_auto_from_rs, Data, Reader};
use std::io::Cursor;

fn truncate_footer(truncated: bool) -> String {
    if truncated {
        "\n\n… [truncated]".to_string()
    } else {
        String::new()
    }
}

/// Delimited text: first row as header, then tab-separated cells per row.
pub fn extract_delimited(
    bytes: &[u8],
    delimiter: u8,
    max_rows: usize,
    max_cols: usize,
) -> Result<String> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .from_reader(std::io::Cursor::new(bytes.to_vec()));
    let mut out = String::new();
    let mut truncated = false;
    let mut row_count = 0usize;
    for result in rdr.records() {
        if row_count >= max_rows {
            truncated = true;
            break;
        }
        let rec = result.context("csv parse")?;
        let mut cells: Vec<&str> = Vec::new();
        for (i, f) in rec.iter().enumerate() {
            if i >= max_cols {
                break;
            }
            cells.push(f.trim());
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&cells.join("\t"));
        row_count += 1;
    }
    out.push_str(&truncate_footer(truncated));
    Ok(out)
}

pub fn extract_jsonl(bytes: &[u8], max_rows: usize) -> Result<String> {
    let s = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    let mut truncated = false;
    let mut n = 0usize;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if n >= max_rows {
            truncated = true;
            break;
        }
        let v: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|_| serde_json::json!({"raw": line}));
        let pretty = serde_json::to_string(&v).unwrap_or_else(|_| line.to_string());
        if !out.is_empty() {
            out.push_str("\n---\n");
        }
        out.push_str(&pretty);
        n += 1;
    }
    out.push_str(&truncate_footer(truncated));
    Ok(out)
}

pub fn extract_excel(bytes: &[u8], max_rows: usize, max_cols: usize) -> Result<String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut workbook = open_workbook_auto_from_rs(cursor).context("open workbook")?;
    let sheet_names = workbook.sheet_names().to_vec();
    let mut out = String::new();
    let mut truncated = false;

    'sheets: for name in sheet_names {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str("## ");
        out.push_str(&name);
        out.push('\n');

        let range = match workbook.worksheet_range(&name) {
            Ok(r) => r,
            Err(_) => continue,
        };

        let height = range.height();
        let width = range.width().min(max_cols);
        let mut rows_emitted = 0usize;
        for row in 0..height {
            if rows_emitted >= max_rows {
                truncated = true;
                break 'sheets;
            }
            let mut cells: Vec<String> = Vec::new();
            for col in 0..width {
                let cell = range.get((row, col)).unwrap_or(&Data::Empty);
                cells.push(cell_to_string(cell));
            }
            if cells.iter().all(|c| c.trim().is_empty()) {
                continue;
            }
            out.push_str(&cells.join("\t"));
            out.push('\n');
            rows_emitted += 1;
        }
    }
    out.push_str(&truncate_footer(truncated));
    Ok(out)
}

fn cell_to_string(c: &Data) -> String {
    match c {
        Data::Empty => String::new(),
        Data::String(s) => s.clone(),
        Data::Float(f) => f.to_string(),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::Error(e) => format!("{e:?}"),
        Data::DateTime(dt) => dt.to_string(),
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_three_rows() {
        let b = b"a,b\n1,2\n3,4\n";
        let t = extract_delimited(b, b',', 100, 32).unwrap();
        assert!(t.contains("1\t2"), "unexpected csv output: {t:?}");
        assert!(t.contains("3\t4"), "unexpected csv output: {t:?}");
    }

    #[test]
    fn jsonl_truncates() {
        let mut s = String::new();
        for i in 0..5 {
            s.push_str(&format!("{{\"i\":{}}}\n", i));
        }
        let t = extract_jsonl(s.as_bytes(), 3).unwrap();
        assert!(t.contains("[truncated]"));
        assert!(t.matches("---").count() >= 2);
    }
}
