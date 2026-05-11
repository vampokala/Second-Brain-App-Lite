//! Simple BM25-style retrieval over wiki markdown files.

use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetrievalHit {
    pub path: String,
    pub score: f64,
    pub excerpt: String,
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|t| t.len() > 1)
        .map(|s| s.to_string())
        .collect()
}

fn term_freq(tokens: &[String]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for t in tokens {
        *m.entry(t.clone()).or_insert(0) += 1;
    }
    m
}

/// lite BM25 over filename + first chunk of body
pub fn search(wiki_root: &Path, query: &str, limit: usize, track_filter: Option<&str>) -> Vec<RetrievalHit> {
    let q_terms = tokenize(query);
    if q_terms.is_empty() {
        return vec![];
    }

    let mut docs: Vec<(PathBuf, String)> = vec![];
    for entry in WalkDir::new(wiki_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(p) else {
            continue;
        };
        let rel = p.strip_prefix(wiki_root).unwrap_or(p);
        let name = rel.to_string_lossy().to_string();
        if let Some(tf) = track_filter {
            let tf = tf.trim();
            if !tf.is_empty() && !name.starts_with(&format!("sources/{}/", tf)) {
                continue;
            }
        }
        let excerpt_slice: String = body.chars().take(3500).collect();
        docs.push((rel.to_path_buf(), format!("{} {}", name, excerpt_slice)));
    }

    let n = docs.len().max(1) as f64;
    let k1 = 1.2_f64;
    let b = 0.75_f64;

    let avg_dl = docs.iter().map(|(_, d)| tokenize(d).len() as f64).sum::<f64>() / n;

    let mut df = HashMap::<String, usize>::new();
    for (_, text) in &docs {
        let tf = term_freq(&tokenize(text));
        for t in tf.keys() {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
    }

    let mut idf = HashMap::new();
    for t in &q_terms {
        let dfi = *df.get(t).unwrap_or(&0) as f64;
        idf.insert(
            t.clone(),
            ((n - dfi + 0.5) / (dfi + 0.5) + 1.0).ln(),
        );
    }

    let mut scored = vec![];
    for (rel, text) in docs {
        let tokens = tokenize(&text);
        let dl = tokens.len() as f64;
        let tf = term_freq(&tokens);
        let mut score = 0_f64;
        for t in &q_terms {
            let f = *tf.get(t).unwrap_or(&0) as f64;
            let idf_t = *idf.get(t).unwrap_or(&0.0);
            let denom = f + k1 * (1.0 - b + b * dl / avg_dl.max(1.0));
            score += idf_t * (f * (k1 + 1.0)) / denom.max(1e-9);
        }
        if score > 0.0 {
            let excerpt = text.chars().take(600).collect::<String>();
            scored.push(RetrievalHit {
                path: rel.to_string_lossy().into_owned(),
                score,
                excerpt,
            });
        }
    }

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(limit);
    scored
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_track_filter() {
        let root = std::env::temp_dir().join(format!(
            "sb-lite-retrieval-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(root.join("sources/claims")).unwrap();
        std::fs::create_dir_all(root.join("sources/sales")).unwrap();
        std::fs::write(root.join("sources/claims/a.md"), "denial workflow details").unwrap();
        std::fs::write(root.join("sources/sales/b.md"), "pipeline forecast details").unwrap();
        let hits = search(&root, "workflow", 8, Some("claims"));
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path.starts_with("sources/claims/"));
        let _ = std::fs::remove_dir_all(root);
    }
}
