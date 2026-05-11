//! Plain-text extraction from raw files for ingest (Markdown, PDF, Word, HTML).

use anyhow::{Context, Result};
use docx::document::{BodyContent, Paragraph, TableCellContent};
use docx::DocxFile;
use std::io::Cursor;
use std::path::Path;

/// Extensions scanned under `raw/` for ingest (case-insensitive).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "pdf", "docx", "html", "htm",
];

pub fn is_supported_raw_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let lower = e.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&lower.as_str())
        })
        .unwrap_or(false)
}

/// Produce UTF-8 plain text suitable for the ingest LLM prompt.
pub fn extract_plain_text(path: &Path, bytes: &[u8]) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let text = match ext.as_str() {
        "md" | "markdown" | "txt" => String::from_utf8_lossy(bytes).into_owned(),
        "html" | "htm" => html2text::from_read(Cursor::new(bytes), 120)
            .map_err(|e| anyhow::anyhow!("HTML parse: {}", e))?,
        "pdf" => pdf_extract::extract_text_from_mem(bytes).context("extract PDF text")?,
        "docx" => extract_docx_plain(bytes)?,
        _ => anyhow::bail!(
            "unsupported file extension {:?} (supported: {})",
            path.extension(),
            SUPPORTED_EXTENSIONS.join(", ")
        ),
    };

    Ok(normalize_extracted_text(&text))
}

fn normalize_extracted_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_blank = false;
    for line in s.lines() {
        let t = line.trim_end();
        let blank = t.trim().is_empty();
        if blank {
            if !prev_blank {
                out.push('\n');
            }
            prev_blank = true;
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(t);
            out.push('\n');
            prev_blank = false;
        }
    }
    out.trim().to_string()
}

fn paragraph_plain(p: &Paragraph<'_>) -> String {
    p.iter_text().map(|c| c.as_ref()).collect::<Vec<_>>().concat()
}

fn extract_docx_plain(bytes: &[u8]) -> Result<String> {
    let file = DocxFile::from_reader(Cursor::new(bytes))
        .map_err(|e| anyhow::anyhow!("read docx archive: {:?}", e))?;
    let doc = file
        .parse()
        .map_err(|e| anyhow::anyhow!("parse docx XML: {:?}", e))?;
    Ok(collect_body_text(&doc.document.body.content))
}

fn collect_body_text(contents: &[BodyContent<'_>]) -> String {
    let mut blocks = Vec::new();
    for item in contents {
        match item {
            BodyContent::Paragraph(p) => {
                let line = paragraph_plain(p);
                if !line.trim().is_empty() {
                    blocks.push(line);
                }
            }
            BodyContent::Table(t) => {
                for row in &t.rows {
                    let mut cells = Vec::new();
                    for cell in &row.cells {
                        let mut cell_parts = Vec::new();
                        for tc in &cell.content {
                            let TableCellContent::Paragraph(p) = tc;
                            let s = paragraph_plain(p);
                            if !s.trim().is_empty() {
                                cell_parts.push(s);
                            }
                        }
                        if !cell_parts.is_empty() {
                            cells.push(cell_parts.join(" "));
                        }
                    }
                    if !cells.is_empty() {
                        blocks.push(cells.join("\t"));
                    }
                }
            }
        }
    }
    blocks.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn html_extracts_visible_text() {
        let html = b"<html><body><p>Hello <strong>world</strong></p></body></html>";
        let t = extract_plain_text(Path::new("sample.html"), html).unwrap();
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
    }

    #[test]
    fn markdown_passes_through() {
        let md = b"# Title\n\nBody **bold**.";
        let t = extract_plain_text(Path::new("n.md"), md).unwrap();
        assert!(t.contains("Title"));
        assert!(t.contains("bold"));
    }

    #[test]
    fn unsupported_extension_errors() {
        let r = extract_plain_text(Path::new("x.xyz"), b"hi");
        assert!(r.is_err());
    }
}
