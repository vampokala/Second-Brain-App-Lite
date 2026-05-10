use crate::atomic;
use crate::paths::user_data_dir;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(default)]
    pub ts: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub created: String,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub messages: Vec<ChatMessage>,
}

fn sessions_dir() -> Result<PathBuf> {
    Ok(user_data_dir()?.join("sessions"))
}

pub fn list_sessions() -> Result<Vec<SessionFile>> {
    let dir = sessions_dir()?;
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for e in std::fs::read_dir(&dir)? {
        let e = e?;
        let p = e.path();
        if p.extension().map(|x| x == "json").unwrap_or(false) {
            if let Ok(s) = std::fs::read_to_string(&p) {
                if let Ok(sess) = serde_json::from_str::<SessionFile>(&s) {
                    out.push(sess);
                }
            }
        }
    }
    out.sort_by(|a, b| b.updated.cmp(&a.updated));
    Ok(out)
}

pub fn load_session(id: &str) -> Result<SessionFile> {
    let p = sessions_dir()?.join(format!("{}.json", id));
    let s = std::fs::read_to_string(&p).with_context(|| format!("read {}", p.display()))?;
    Ok(serde_json::from_str(&s)?)
}

pub fn save_session(sess: &SessionFile) -> Result<()> {
    let dir = sessions_dir()?;
    std::fs::create_dir_all(&dir)?;
    let p = dir.join(format!("{}.json", sess.id));
    let json = serde_json::to_string_pretty(sess)?;
    atomic::atomic_write(&p, json.as_bytes())
}

pub fn delete_session(id: &str) -> Result<()> {
    let p = sessions_dir()?.join(format!("{}.json", id));
    let _ = std::fs::remove_file(p);
    Ok(())
}
