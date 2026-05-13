//! PowerPoint `.pptx` → plain text (slide text + speaker notes).

use anyhow::{Context, Result};
use regex::Regex;
use std::io::{Cursor, Read};
use zip::ZipArchive;

fn collect_slide_text(xml: &str) -> String {
    let re_a = Regex::new(r"<a:t[^>]*>([^<]*)</a:t>").expect("regex");
    let re_w = Regex::new(r"<w:t[^>]*>([^<]*)</w:t>").expect("regex");
    let mut parts: Vec<String> = Vec::new();
    for c in re_a.captures_iter(xml) {
        if let Some(m) = c.get(1) {
            let s = m.as_str().trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    for c in re_w.captures_iter(xml) {
        if let Some(m) = c.get(1) {
            let s = m.as_str().trim();
            if !s.is_empty() {
                parts.push(s.to_string());
            }
        }
    }
    parts.join("\n")
}

pub fn extract_pptx(bytes: &[u8]) -> Result<String> {
    let cursor = Cursor::new(bytes.to_vec());
    let mut zip = ZipArchive::new(cursor).context("pptx zip")?;
    let mut names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let n = zip.by_index(i).map(|f| f.name().to_string());
        if let Ok(name) = n {
            names.push(name);
        }
    }

    let mut slide_nums: Vec<u32> = Vec::new();
    for n in &names {
        if let Some(rest) = n.strip_prefix("ppt/slides/slide") {
            if let Some(num) = rest.strip_suffix(".xml") {
                if let Ok(idx) = num.parse::<u32>() {
                    slide_nums.push(idx);
                }
            }
        }
    }
    slide_nums.sort_unstable();
    slide_nums.dedup();

    let mut out = String::new();
    for n in slide_nums {
        let slide_path = format!("ppt/slides/slide{n}.xml");
        let notes_path = format!("ppt/notesSlides/notesSlide{n}.xml");
        let slide_xml = read_zip_by_name(&mut zip, &slide_path).unwrap_or_default();
        let notes_xml = read_zip_by_name(&mut zip, &notes_path).unwrap_or_default();

        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("## Slide {n}\n"));
        let body = collect_slide_text(&slide_xml);
        if !body.is_empty() {
            out.push_str(&body);
            out.push('\n');
        }
        let notes = collect_slide_text(&notes_xml);
        if !notes.is_empty() {
            out.push_str("Notes: ");
            out.push_str(&notes);
            out.push('\n');
        }
    }
    Ok(out.trim().to_string())
}

fn read_zip_by_name(zip: &mut ZipArchive<Cursor<Vec<u8>>>, path: &str) -> Result<String> {
    let mut f = zip.by_name(path).context("zip by_name")?;
    let mut buf = String::new();
    f.read_to_string(&mut buf).context("read xml")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_text_runs() {
        let xml = r#"<xml><a:t>Hello</a:t><w:t xml:space="preserve"> world</w:t></xml>"#;
        let t = collect_slide_text(xml);
        assert!(t.contains("Hello"));
        assert!(t.contains("world"));
    }
}
