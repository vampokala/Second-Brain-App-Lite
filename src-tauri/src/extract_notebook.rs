//! Jupyter `.ipynb` → plain text (sources + capped stream text output).

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Notebook {
    cells: Vec<Cell>,
}

#[derive(Debug, Deserialize)]
struct Cell {
    #[serde(rename = "cell_type")]
    cell_type: String,
    source: serde_json::Value,
    #[serde(default)]
    outputs: Vec<serde_json::Value>,
}

fn source_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => v.to_string(),
    }
}

fn append_capped_lines(out: &mut String, text: &str, lines_used: &mut usize, max_lines: usize) {
    for line in text.lines() {
        if *lines_used >= max_lines {
            out.push_str("\n… [output truncated]\n");
            return;
        }
        out.push_str(line);
        out.push('\n');
        *lines_used += 1;
    }
}

pub fn extract_ipynb(bytes: &[u8], max_output_lines: usize) -> Result<String> {
    let nb: Notebook = serde_json::from_slice(bytes).context("parse ipynb json")?;
    let mut out = String::new();
    for (i, cell) in nb.cells.iter().enumerate() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("### Cell {} ({})\n", i + 1, cell.cell_type));
        let src = source_to_string(&cell.source);
        out.push_str(&src);
        if cell.cell_type == "code" && !cell.outputs.is_empty() {
            out.push_str("\n\n#### Output (capped)\n");
            let mut lines = 0usize;
            for o in &cell.outputs {
                if lines >= max_output_lines {
                    break;
                }
                if let Some(text) = o.get("text") {
                    match text {
                        serde_json::Value::String(s) => {
                            append_capped_lines(&mut out, s, &mut lines, max_output_lines);
                        }
                        serde_json::Value::Array(a) => {
                            let joined = a
                                .iter()
                                .filter_map(|x| x.as_str())
                                .collect::<Vec<_>>()
                                .join("");
                            append_capped_lines(&mut out, &joined, &mut lines, max_output_lines);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_notebook() {
        let j = br#"{"cells":[{"cell_type":"markdown","source":["Hello"]},{"cell_type":"code","source":["print(1)"],"outputs":[]}],"nbformat":4,"nbformat_minor":5,"metadata":{}}"#;
        let t = extract_ipynb(j, 10).unwrap();
        assert!(t.contains("Cell 1"));
        assert!(t.contains("markdown"));
        assert!(t.contains("Hello"));
    }
}
