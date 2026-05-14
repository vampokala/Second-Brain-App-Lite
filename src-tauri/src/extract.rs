//! Plain-text / diagram / tabular extraction and routing for ingest.

use crate::config::AppConfig;
use crate::extract_diagrams;
use crate::extract_image;
use crate::extract_notebook;
use crate::extract_pptx;
use crate::extract_tabular;
use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::read::ZipArchive;

const MAX_TABULAR_COLS: usize = 32;
const IPYNB_OUTPUT_LINES: usize = 40;

/// Extensions scanned under `raw/` for ingest (case-insensitive).
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "md",
    "markdown",
    "mdx",
    "txt",
    "pdf",
    "docx",
    "html",
    "htm",
    "csv",
    "tsv",
    "jsonl",
    "ndjson",
    "xlsx",
    "xlsm",
    "xlsb",
    "xls",
    "ods",
    "sql",
    "dbml",
    "prisma",
    "yaml",
    "yml",
    "json",
    "toml",
    "tf",
    "hcl",
    "mmd",
    "mermaid",
    "puml",
    "plantuml",
    "iuml",
    "pu",
    "excalidraw",
    "drawio",
    "dio",
    "svg",
    "ipynb",
    "pptx",
    "png",
    "jpg",
    "jpeg",
    "webp",
    "gif",
    "bmp",
    "tif",
    "tiff",
    "py",
    "ts",
    "tsx",
    "js",
    "jsx",
    "go",
    "java",
    "rs",
    "rb",
    "kt",
    "swift",
    "cpp",
    "cc",
    "h",
    "hpp",
    "c",
    "cs",
    "scala",
    "php",
    "sh",
    "bash",
    "ps1",
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

#[derive(Debug, Clone)]
pub enum IngestPayload {
    Text(String),
    Image(extract_image::IngestImage),
    /// Vision ingest skipped (caller should record as `skipped` in manifest flow).
    Skipped(String),
}

pub fn truncate_utf8_bytes(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n\n… [truncated]", &s[..end])
}

/// Route raw bytes to text extraction, raster vision payload, or an explicit skip.
pub fn classify_for_ingest(path: &Path, bytes: &[u8], cfg: &AppConfig) -> Result<IngestPayload> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let cap = cfg.text_max_bytes;
    let tab_rows = cfg.tabular_max_rows;

    let text_payload = |s: String| {
        let mut s = normalize_extracted_text(&s);
        if ext == "tf" || ext == "hcl" {
            s = format!("(IaC / config file)\n\n{s}");
        }
        Ok(IngestPayload::Text(truncate_utf8_bytes(&s, cap)))
    };

    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "tif" | "tiff" => {
            if !cfg.vision_enabled {
                return Ok(IngestPayload::Skipped(
                    "vision ingest disabled in settings".into(),
                ));
            }
            let img = extract_image::prepare_image_for_vision(
                bytes,
                cfg.vision_max_bytes,
                cfg.vision_max_edge_px,
            )?;
            Ok(IngestPayload::Image(img))
        }

        "svg" => {
            let txt = extract_diagrams::extract_svg_text(bytes)?;
            text_payload(txt)
        }

        "csv" => {
            let s = extract_tabular::extract_delimited(bytes, b',', tab_rows, MAX_TABULAR_COLS)?;
            text_payload(s)
        }
        "tsv" => {
            let s = extract_tabular::extract_delimited(bytes, b'\t', tab_rows, MAX_TABULAR_COLS)?;
            text_payload(s)
        }
        "jsonl" | "ndjson" => {
            let s = extract_tabular::extract_jsonl(bytes, tab_rows)?;
            text_payload(s)
        }
        "xlsx" | "xlsm" | "xlsb" | "xls" | "ods" => {
            let s = extract_tabular::extract_excel(bytes, tab_rows, MAX_TABULAR_COLS)?;
            text_payload(s)
        }

        "drawio" | "dio" => {
            let s = extract_diagrams::extract_drawio(bytes)?;
            text_payload(s)
        }
        "excalidraw" => {
            let s = extract_diagrams::extract_excalidraw(bytes)?;
            text_payload(s)
        }

        "ipynb" => {
            let s = extract_notebook::extract_ipynb(bytes, IPYNB_OUTPUT_LINES)?;
            text_payload(s)
        }
        "pptx" => {
            let s = extract_pptx::extract_pptx(bytes)?;
            text_payload(s)
        }

        "sql"
        | "dbml"
        | "prisma"
        | "yaml"
        | "yml"
        | "json"
        | "toml"
        | "tf"
        | "hcl"
        | "mmd"
        | "mermaid"
        | "puml"
        | "plantuml"
        | "iuml"
        | "pu"
        | "py"
        | "ts"
        | "tsx"
        | "js"
        | "jsx"
        | "go"
        | "java"
        | "rs"
        | "rb"
        | "kt"
        | "swift"
        | "cpp"
        | "cc"
        | "h"
        | "hpp"
        | "c"
        | "cs"
        | "scala"
        | "php"
        | "sh"
        | "bash"
        | "ps1" => {
            let s = String::from_utf8_lossy(bytes).into_owned();
            text_payload(s)
        }

        _ => {
            let s = extract_plain_text_inner(path, bytes)?;
            Ok(IngestPayload::Text(truncate_utf8_bytes(&s, cap)))
        }
    }
}

/// Produce UTF-8 plain text suitable for the ingest LLM prompt (legacy / HTML URL path).
pub fn extract_plain_text(path: &Path, bytes: &[u8]) -> Result<String> {
    let s = extract_plain_text_inner(path, bytes)?;
    Ok(normalize_extracted_text(&s))
}

fn extract_plain_text_inner(path: &Path, bytes: &[u8]) -> Result<String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let text = match ext.as_str() {
        "md" | "markdown" | "mdx" | "txt" => String::from_utf8_lossy(bytes).into_owned(),
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

/// Plain text from a `.docx` (OOXML) using `word/document.xml` — avoids the unmaintained `docx`
/// crate on crates.io (future-incompatibility warnings under newer Rust).
fn extract_docx_plain(bytes: &[u8]) -> Result<String> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|e| anyhow::anyhow!("read docx zip: {}", e))?;
    let mut xml = String::new();
    {
        let mut f = archive
            .by_name("word/document.xml")
            .map_err(|e| anyhow::anyhow!("word/document.xml: {}", e))?;
        f.read_to_string(&mut xml)
            .map_err(|e| anyhow::anyhow!("read document.xml: {}", e))?;
    }

    let mut reader = Reader::from_str(&xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    // Stack: true when the open element is `w:t` (text).
    let mut stack: Vec<bool> = Vec::new();
    let mut out = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                match e.local_name().as_ref() {
                    b"tab" => out.push('\t'),
                    b"br" => out.push('\n'),
                    _ => {}
                }
                stack.push(e.local_name().as_ref() == b"t");
            }
            Ok(Event::Empty(ref e)) => match e.local_name().as_ref() {
                b"tab" => out.push('\t'),
                b"br" => out.push('\n'),
                _ => {}
            },
            Ok(Event::Text(ref e)) => {
                if stack.last().copied().unwrap_or(false) {
                    let txt = e
                        .unescape()
                        .map_err(|err| anyhow::anyhow!("xml text: {}", err))?;
                    out.push_str(&txt);
                }
            }
            Ok(Event::End(ref e)) => {
                if e.local_name().as_ref() == b"p" {
                    if !out.is_empty() && !out.ends_with('\n') {
                        out.push('\n');
                    }
                    out.push('\n');
                }
                let _ = stack.pop();
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("word/document.xml parse: {}", e),
            Ok(_) => {}
        }
        buf.clear();
    }

    Ok(normalize_extracted_text(&out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

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

    #[test]
    fn truncate_utf8() {
        let s = "abcde";
        assert_eq!(truncate_utf8_bytes(s, 10), s);
    }

    #[test]
    fn classify_csv_to_text() {
        let cfg = AppConfig::default();
        let p = classify_for_ingest(Path::new("t.csv"), b"a,b\n1,2", &cfg).unwrap();
        match p {
            IngestPayload::Text(t) => assert!(t.contains("1\t2")),
            _ => panic!("expected text payload"),
        }
    }

    #[test]
    fn classify_png_skipped_when_vision_off() {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00,
            0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63,
            0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00,
            0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let mut cfg = AppConfig::default();
        cfg.vision_enabled = false;
        let p = classify_for_ingest(Path::new("x.png"), PNG, &cfg).unwrap();
        assert!(matches!(p, IngestPayload::Skipped(_)));
    }
}
