//! Discover Cursor `workspaceStorage` JSONL transcripts (MVP: no `state.vscdb` parsing).

use anyhow::{Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorWorkspaceEntry {
    pub id: String,
    pub workspace_hash: String,
    pub project_slug: String,
    pub abs_path: String,
    pub modified_ms: Option<i64>,
    pub folder_hint: Option<String>,
    pub has_state_vscdb: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorTranscriptFile {
    pub name: String,
    pub abs_path: String,
    pub modified_ms: Option<i64>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatExcerptMessage {
    pub role: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatExcerpt {
    pub title: String,
    pub source_path: String,
    pub messages: Vec<ChatExcerptMessage>,
    pub truncated: bool,
}

fn cursor_workspace_storage_root() -> Result<PathBuf> {
    if cfg!(target_os = "macos") {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join("Library/Application Support/Cursor/User/workspaceStorage"));
        }
    } else if cfg!(target_os = "linux") {
        if let Some(home) = dirs::home_dir() {
            return Ok(home.join(".config/Cursor/User/workspaceStorage"));
        }
    } else if cfg!(target_os = "windows") {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return Ok(PathBuf::from(appdata).join("Cursor/User/workspaceStorage"));
        }
    }
    anyhow::bail!("could not resolve Cursor workspaceStorage path for this OS")
}

pub fn project_slug_from_hash(hash: &str) -> String {
    let h: String = hash.chars().take(12).collect();
    format!("cursor-{h}")
}

fn read_workspace_folder_hint(dir: &Path) -> Option<String> {
    let wj = dir.join("workspace.json");
    let s = std::fs::read_to_string(wj).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v.get("folder")
        .and_then(|x| x.as_str())
        .map(|s| s.to_string())
        .or_else(|| {
            v.get("workspace")
                .and_then(|w| w.get("folder"))
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
}

/// List Cursor workspace hash folders under the OS-specific workspaceStorage root.
pub fn discover_workspaces() -> Result<Vec<CursorWorkspaceEntry>> {
    let root = cursor_workspace_storage_root()?;
    if !root.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for e in std::fs::read_dir(&root).with_context(|| format!("read_dir {}", root.display()))? {
        let e = e?;
        let ft = e.file_type()?;
        if !ft.is_dir() {
            continue;
        }
        let hash = e.file_name().to_string_lossy().to_string();
        if hash.starts_with('.') {
            continue;
        }
        let dir = e.path();
        let meta = std::fs::metadata(&dir).ok();
        let modified_ms = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis().min(i64::MAX as u128) as i64);
        let has_state_vscdb = dir.join("state.vscdb").exists();
        let folder_hint = read_workspace_folder_hint(&dir);
        out.push(CursorWorkspaceEntry {
            id: hash.clone(),
            workspace_hash: hash.clone(),
            project_slug: project_slug_from_hash(&hash),
            abs_path: dir.to_string_lossy().to_string(),
            modified_ms,
            folder_hint,
            has_state_vscdb,
        });
    }
    out.sort_by(|a, b| {
        b.modified_ms
            .unwrap_or(0)
            .cmp(&a.modified_ms.unwrap_or(0))
    });
    Ok(out)
}

#[derive(Debug, Clone, Default)]
pub struct WorkspaceFilter {
    pub hash_contains: Option<String>,
    pub max_age_days: Option<u32>,
    pub vault_path_contains: Option<String>,
}

pub fn filter_workspaces(entries: Vec<CursorWorkspaceEntry>, f: &WorkspaceFilter) -> Vec<CursorWorkspaceEntry> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let max_ms = f.max_age_days.map(|d| (d as i64) * 86_400_000);
    entries
        .into_iter()
        .filter(|e| {
            if let Some(ref needle) = f.hash_contains {
                if !e.workspace_hash.to_lowercase().contains(&needle.to_lowercase()) {
                    return false;
                }
            }
            if let (Some(max_age), Some(mod_ms)) = (max_ms, e.modified_ms) {
                if now_ms - mod_ms > max_age {
                    return false;
                }
            }
            if let Some(ref v) = f.vault_path_contains {
                let vlow = v.to_lowercase();
                let hint = e
                    .folder_hint
                    .as_ref()
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                if !hint.contains(&vlow) {
                    return false;
                }
            }
            true
        })
        .collect()
}

/// List `*.jsonl` files under a workspace hash directory (shallow walk).
pub fn list_transcripts(workspace_abs: &str) -> Result<Vec<CursorTranscriptFile>> {
    let dir = PathBuf::from(workspace_abs);
    if !dir.is_dir() {
        anyhow::bail!("not a directory: {}", workspace_abs);
    }
    let mut files = vec![];
    for entry in WalkDir::new(&dir)
        .max_depth(6)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }
        let meta = std::fs::metadata(p).ok();
        let modified_ms = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis().min(i64::MAX as u128) as i64);
        let size_bytes = meta.map(|m| m.len()).unwrap_or(0);
        files.push(CursorTranscriptFile {
            name: p
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            abs_path: p.to_string_lossy().to_string(),
            modified_ms,
            size_bytes,
        });
    }
    files.sort_by(|a, b| {
        b.modified_ms
            .unwrap_or(0)
            .cmp(&a.modified_ms.unwrap_or(0))
    });
    Ok(files)
}

fn extract_line_text(v: &serde_json::Value) -> Option<(String, String)> {
    if let (Some(r), Some(c)) = (v.get("role").and_then(|x| x.as_str()), v.get("content")) {
        let text = if let Some(s) = c.as_str() {
            s.to_string()
        } else if let Some(arr) = c.as_array() {
            arr.iter()
                .filter_map(|part| part.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            return None;
        };
        if text.trim().is_empty() {
            return None;
        }
        return Some((r.to_string(), text));
    }
    if let Some(typ) = v.get("type").and_then(|x| x.as_str()) {
        let role = match typ {
            "userMessage" | "user" => "user",
            "assistant" | "model" | "bot" | "ai" => "assistant",
            _ => typ,
        };
        if let Some(content) = v
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
        {
            return Some((role.to_string(), content.to_string()));
        }
        if let Some(text) = v.get("text").and_then(|t| t.as_str()) {
            return Some((role.to_string(), text.to_string()));
        }
    }
    None
}

/// Parse NDJSON transcript into a bounded excerpt for UI / prompt enrichment.
pub fn parse_excerpt(path: &str, max_chars: usize) -> Result<ChatExcerpt> {
    let p = Path::new(path);
    let s = std::fs::read_to_string(p)
        .with_context(|| format!("read transcript {}", path))?;
    let mut messages = vec![];
    let mut consumed = 0usize;
    let mut truncated = false;
    for line in s.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some((role, text)) = extract_line_text(&v) {
            let remain = max_chars.saturating_sub(consumed);
            if remain == 0 {
                truncated = true;
                break;
            }
            let slice = if text.len() <= remain {
                text
            } else {
                truncated = true;
                text.chars().take(remain).collect()
            };
            consumed += slice.len();
            messages.push(ChatExcerptMessage {
                role,
                text: slice,
            });
        }
    }
    let title = format!(
        "Cursor {}",
        p.file_name().unwrap_or_default().to_string_lossy()
    );
    Ok(ChatExcerpt {
        title,
        source_path: path.to_string(),
        messages,
        truncated,
    })
}

pub fn render_excerpt_markdown(ex: &ChatExcerpt) -> String {
    let mut md = String::new();
    md.push_str(&format!("## {}\n\n", ex.title));
    md.push_str("_Imported from local Cursor transcript._\n\n---\n\n");
    for m in &ex.messages {
        md.push_str(&format!("### {}\n\n{}\n\n", m.role, m.text));
    }
    if ex.truncated {
        md.push_str("\n_(Excerpt truncated for size.)_\n");
    }
    md
}

/// Reveal file in Finder (macOS), Explorer (Windows), or open parent in xdg-open (Linux).
pub fn reveal_path_in_os(path: &str) -> Result<()> {
    let p = Path::new(path);
    if cfg!(target_os = "macos") {
        let st = std::process::Command::new("open")
            .arg("-R")
            .arg(p)
            .status()
            .context("spawn open")?;
        if !st.success() {
            anyhow::bail!("open -R failed with {:?}", st.code());
        }
    } else if cfg!(target_os = "windows") {
        let st = std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.display()))
            .status()
            .context("spawn explorer")?;
        if !st.success() {
            anyhow::bail!("explorer failed with {:?}", st.code());
        }
    } else {
        let parent = p.parent().unwrap_or(p);
        let st = std::process::Command::new("xdg-open")
            .arg(parent)
            .status()
            .context("spawn xdg-open")?;
        if !st.success() {
            anyhow::bail!("xdg-open failed with {:?}", st.code());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_fixture() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../tests/fixtures/cursor/minimal.jsonl"
        );
        let ex = parse_excerpt(path, 50_000).expect("parse");
        assert_eq!(ex.messages.len(), 2);
        assert_eq!(ex.messages[0].role, "user");
        assert!(ex.messages[1].text.contains("Cursor-assisted"));
    }
}
