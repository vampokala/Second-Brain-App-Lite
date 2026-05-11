//! API keys stored in an AES-256-GCM encrypted JSON file.
//!
//! Layout under `{user_data_dir}/`:
//!   secrets.key  — 32-byte random AES key (created once, chmod 600 on Unix)
//!   secrets.enc  — JSON envelope: { "nonce": "<b64>", "ciphertext": "<b64>" }
//!                  The plaintext is a JSON object mapping account → secret.
//!
//! Env vars (OPENAI_API_KEY etc.) always override stored values.

use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const KEY_FILE: &str = "secrets.key";
const SECRETS_FILE: &str = "secrets.enc";

// ── paths ─────────────────────────────────────────────────────────────────────

fn key_path() -> Result<std::path::PathBuf> {
    Ok(crate::paths::user_data_dir()?.join(KEY_FILE))
}

fn secrets_path() -> Result<std::path::PathBuf> {
    Ok(crate::paths::user_data_dir()?.join(SECRETS_FILE))
}

// ── key management ────────────────────────────────────────────────────────────

fn load_or_create_key() -> Result<Vec<u8>> {
    let path = key_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create app data dir")?;
    }

    if path.exists() {
        let bytes = std::fs::read(&path).context("read secrets.key")?;
        if bytes.len() == 32 {
            return Ok(bytes);
        }
        // Corrupt/wrong size — regenerate
    }

    let key = Aes256Gcm::generate_key(OsRng);
    std::fs::write(&path, key.as_slice()).context("write secrets.key")?;

    // Unix: restrict to owner-read/write only (rw-------)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .context("chmod secrets.key")?;
    }
    // Windows: AppData\Local is already protected by NTFS ACLs so that only
    // the current user (and SYSTEM/Administrators) can access it — no extra
    // permission step needed.

    Ok(key.to_vec())
}

// ── encrypted envelope ────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct Envelope {
    nonce: String,
    ciphertext: String,
}

fn decrypt_map() -> Result<HashMap<String, String>> {
    let path = secrets_path()?;
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read(&path).context("read secrets.enc")?;
    let env: Envelope = serde_json::from_slice(&raw).context("parse secrets envelope")?;

    let key_bytes = load_or_create_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);

    let nonce_bytes = B64.decode(&env.nonce).context("decode nonce")?;
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = B64.decode(&env.ciphertext).context("decode ciphertext")?;

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow!("Decryption failed — secrets.key may have been replaced"))?;

    serde_json::from_slice(&plaintext).context("parse secrets JSON")
}

fn encrypt_map(map: &HashMap<String, String>) -> Result<()> {
    let key_bytes = load_or_create_key()?;
    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    let cipher = Aes256Gcm::new(key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);

    let plaintext = serde_json::to_vec(map).context("serialize secrets")?;
    let ciphertext = cipher
        .encrypt(&nonce, plaintext.as_ref())
        .map_err(|_| anyhow!("Encryption failed"))?;

    let env = Envelope {
        nonce: B64.encode(nonce),
        ciphertext: B64.encode(ciphertext),
    };

    let path = secrets_path()?;
    crate::atomic::atomic_write(&path, serde_json::to_vec_pretty(&env)?.as_slice())
        .context("write secrets.enc")
}

// ── public API (same signatures as the old keyring version) ──────────────────

pub fn set_secret(account: &str, secret: &str) -> Result<()> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(anyhow!("secret is empty"));
    }
    let mut map = decrypt_map()?;
    map.insert(account.to_owned(), secret.to_owned());
    encrypt_map(&map)
}

pub fn get_secret(account: &str) -> Result<Option<String>> {
    Ok(decrypt_map()?.remove(account))
}

pub fn masked_hint(account: &str) -> Result<Option<String>> {
    Ok(get_secret(account)?.map(|s| {
        let s = s.trim().to_owned();
        if s.len() <= 4 {
            "****".into()
        } else {
            format!("…{}", &s[s.len().saturating_sub(4)..])
        }
    }))
}

// ── env-then-file resolvers (unchanged callers) ───────────────────────────────

fn resolve(env_name: &str, account: &str, missing_detail: &str) -> Result<String> {
    if let Ok(v) = std::env::var(env_name) {
        let t = v.trim().to_owned();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    match get_secret(account)? {
        Some(s) => {
            let t = s.trim().to_owned();
            if t.is_empty() {
                Err(anyhow!(
                    "Saved key for '{}' is empty — paste it again in Settings.",
                    account
                ))
            } else {
                Ok(t)
            }
        }
        None => Err(anyhow!("{}", missing_detail)),
    }
}

pub fn resolve_openai_key() -> Result<String> {
    resolve(
        "OPENAI_API_KEY",
        "openai_api_key",
        "OpenAI key not set — add it in Settings → API Keys.",
    )
}

pub fn resolve_anthropic_key() -> Result<String> {
    resolve(
        "ANTHROPIC_API_KEY",
        "anthropic_api_key",
        "Anthropic key not set — add it in Settings → API Keys.",
    )
}

pub fn resolve_gemini_key() -> Result<String> {
    resolve(
        "GEMINI_API_KEY",
        "gemini_api_key",
        "Gemini key not set — add it in Settings → API Keys.",
    )
}

pub fn resolve_compatible_key() -> Result<String> {
    resolve(
        "COMPATIBLE_API_KEY",
        "compatible_api_key",
        "Compatible API key not set — add it in Settings → API Keys.",
    )
}
