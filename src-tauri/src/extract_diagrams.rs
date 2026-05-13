//! draw.io / Excalidraw / SVG → graph-oriented plain text.

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use regex::Regex;
use std::collections::HashMap;
use std::io::Cursor;

fn strip_html_tags(s: &str) -> String {
    let re = Regex::new(r"<[^>]+>").unwrap();
    re.replace_all(s, "").trim().to_string()
}

/// Parse draw.io / diagrams.net mxGraph XML.
pub fn extract_drawio(bytes: &[u8]) -> Result<String> {
    let mut reader = Reader::from_reader(Cursor::new(bytes));
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut cells: Vec<HashMap<String, String>> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(ref e)) | Ok(Event::Start(ref e)) => {
                if e.local_name().as_ref() == b"mxCell" {
                    let mut m = HashMap::new();
                    for attr in e.attributes() {
                        let a = attr.map_err(|e| anyhow::anyhow!("attr {:?}", e))?;
                        let key = std::str::from_utf8(a.key.local_name().as_ref())
                            .unwrap_or("")
                            .to_string();
                        let v = a
                            .decode_and_unescape_value(reader.decoder())
                            .map(|c| c.into_owned())
                            .unwrap_or_default();
                        m.insert(key, v);
                    }
                    cells.push(m);
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => anyhow::bail!("xml: {}", e),
            _ => {}
        }
        buf.clear();
    }

    let mut id_label: HashMap<String, String> = HashMap::new();
    for c in &cells {
        let id = c.get("id").cloned().unwrap_or_default();
        if id.is_empty() {
            continue;
        }
        let value = c.get("value").map(|s| strip_html_tags(s)).unwrap_or_default();
        if !value.is_empty() {
            id_label.insert(id, value);
        }
    }

    let mut lines: Vec<String> = Vec::new();
    let mut used_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    for c in &cells {
        let edge = c.get("edge").map(|s| s == "1").unwrap_or(false);
        if !edge {
            continue;
        }
        let src = c.get("source").cloned().unwrap_or_default();
        let tgt = c.get("target").cloned().unwrap_or_default();
        if src.is_empty() || tgt.is_empty() {
            continue;
        }
        used_ids.insert(src.clone());
        used_ids.insert(tgt.clone());
        let sl = id_label.get(&src).cloned().unwrap_or_else(|| src.clone());
        let tl = id_label.get(&tgt).cloned().unwrap_or_else(|| tgt.clone());
        let lbl = c.get("value").map(|s| strip_html_tags(s)).unwrap_or_default();
        if lbl.is_empty() {
            lines.push(format!("{sl} --> {tl}"));
        } else {
            lines.push(format!("{sl} --[{lbl}]--> {tl}"));
        }
    }

    for (id, lab) in &id_label {
        if !used_ids.contains(id) && !lab.is_empty() {
            lines.push(format!("(node) {lab}"));
        }
    }

    Ok(lines.join("\n"))
}

pub fn extract_excalidraw(bytes: &[u8]) -> Result<String> {
    let v: serde_json::Value = serde_json::from_slice(bytes).context("excalidraw json")?;
    let elements = v
        .get("elements")
        .and_then(|e| e.as_array())
        .context("elements array")?;

    let mut id_label: HashMap<String, String> = HashMap::new();
    for el in elements {
        let id = el.get("id").and_then(|x| x.as_str()).unwrap_or("");
        if id.is_empty() {
            continue;
        }
        let text = el
            .get("text")
            .and_then(|x| x.as_str())
            .or_else(|| el.get("label").and_then(|x| x.as_str()))
            .unwrap_or("")
            .trim();
        if !text.is_empty() {
            id_label.insert(id.to_string(), text.to_string());
        }
    }

    let mut lines: Vec<String> = Vec::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();

    for el in elements {
        let ty = el.get("type").and_then(|x| x.as_str()).unwrap_or("");
        if ty != "arrow" && ty != "line" {
            continue;
        }
        let start = el
            .pointer("/startBinding/elementId")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        let end = el
            .pointer("/endBinding/elementId")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        if start.is_empty() || end.is_empty() {
            continue;
        }
        used.insert(start.to_string());
        used.insert(end.to_string());
        let sl = id_label.get(start).cloned().unwrap_or_else(|| start.to_string());
        let tl = id_label.get(end).cloned().unwrap_or_else(|| end.to_string());
        lines.push(format!("{sl} --> {tl}"));
    }

    for (id, lab) in &id_label {
        if !used.contains(id) {
            lines.push(format!("(node) {lab}"));
        }
    }

    Ok(lines.join("\n"))
}

/// Best-effort SVG text and link labels (no layout).
pub fn extract_svg_text(bytes: &[u8]) -> Result<String> {
    let s = String::from_utf8_lossy(bytes);
    let mut parts: Vec<String> = Vec::new();
    let re_text = Regex::new(r"<text[^>]*>([^<]*)</text>").unwrap();
    let re_title = Regex::new(r"<title[^>]*>([^<]*)</title>").unwrap();
    for c in re_text.captures_iter(&s) {
        if let Some(m) = c.get(1) {
            let t = m.as_str().trim();
            if !t.is_empty() {
                parts.push(t.to_string());
            }
        }
    }
    for c in re_title.captures_iter(&s) {
        if let Some(m) = c.get(1) {
            let t = m.as_str().trim();
            if !t.is_empty() {
                parts.push(format!("(title) {t}"));
            }
        }
    }
    Ok(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawio_two_nodes_edge() {
        let xml = br#"<?xml version="1.0"?><mxfile><diagram><mxGraphModel><root>
<mxCell id="0"/><mxCell id="1" parent="0"/>
<mxCell id="2" value="A" vertex="1" parent="1"/>
<mxCell id="3" value="B" vertex="1" parent="1"/>
<mxCell id="4" value="rel" edge="1" parent="1" source="2" target="3"/>
</root></mxGraphModel></diagram></mxfile>"#;
        let t = extract_drawio(xml).unwrap();
        assert!(t.contains("A"));
        assert!(t.contains("B"));
    }
}
