//! API keys via OS keychain (keyring). Env vars override when present.

use anyhow::{anyhow, Result};
use keyring::Entry;

const SERVICE: &str = "SecondBrainLite";

#[cfg(target_os = "macos")]
fn macos_keychain_extra_help() -> &'static str {
    "\n\nmacOS: If the system dialog keeps rejecting your password, your **login** keychain password is often **not the same** as your Mac login password anymore (updates, migration, or IT policies). Open **Keychain Access** → select **login** → **File → Change Password for Keychain \"login\"…** and align it with your user password, or follow Apple’s guidance for a damaged login keychain.\n\nWorkaround (no Keychain): quit the app, set the provider API key in your environment (e.g. `export OPENAI_API_KEY=…` in Terminal), start the app from that same Terminal — see README \"Environment variable overrides\"."
}

#[cfg(not(target_os = "macos"))]
fn macos_keychain_extra_help() -> &'static str {
    ""
}

fn keychain_err(operation: &'static str, account: &str, e: keyring::Error) -> anyhow::Error {
    #[cfg(target_os = "macos")]
    {
        anyhow!(
            "Keychain {} failed for '{}' (use your Mac login password on the system dialog unless you changed the login keychain password separately): {}{}",
            operation,
            account,
            e,
            macos_keychain_extra_help()
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        anyhow!("Keychain {} failed for '{}': {}", operation, account, e)
    }
}

pub fn set_secret(account: &str, secret: &str) -> Result<()> {
    let secret = secret.trim();
    if secret.is_empty() {
        return Err(anyhow!("secret is empty"));
    }
    let entry = Entry::new(SERVICE, account)?;
    entry
        .set_password(secret)
        .map_err(|e| keychain_err("save", account, e))?;
    Ok(())
}

pub fn get_secret(account: &str) -> Result<Option<String>> {
    let entry = Entry::new(SERVICE, account)?;
    match entry.get_password() {
        Ok(s) if !s.trim().is_empty() => Ok(Some(s)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(keychain_err("read", account, e)),
    }
}

pub fn masked_hint(account: &str) -> Result<Option<String>> {
    Ok(get_secret(account)?.map(|s| {
        let s = s.trim();
        if s.len() <= 4 {
            "****".into()
        } else {
            format!("…{}", &s[s.len().saturating_sub(4)..])
        }
    }))
}

fn resolve_env_then_keychain(env_name: &str, account: &str, missing_detail: &str) -> Result<String> {
    if let Ok(v) = std::env::var(env_name) {
        let t = v.trim();
        if !t.is_empty() {
            return Ok(t.into());
        }
    }
    match get_secret(account)? {
        Some(s) => {
            let t = s.trim();
            if t.is_empty() {
                Err(anyhow!(
                    "Saved API key for {} is empty after trimming; paste it again and choose Save key.",
                    account
                ))
            } else {
                Ok(t.into())
            }
        }
        None => Err(anyhow!("{}", missing_detail)),
    }
}

pub fn resolve_openai_key() -> Result<String> {
    resolve_env_then_keychain(
        "OPENAI_API_KEY",
        "openai_api_key",
        "OPENAI_API_KEY / saved OpenAI key missing — set the env var or save a key under Configuration.",
    )
}

pub fn resolve_anthropic_key() -> Result<String> {
    resolve_env_then_keychain(
        "ANTHROPIC_API_KEY",
        "anthropic_api_key",
        "ANTHROPIC_API_KEY / saved key missing — set the env var or save a key under Configuration.",
    )
}

pub fn resolve_compatible_key() -> Result<String> {
    resolve_env_then_keychain(
        "COMPATIBLE_API_KEY",
        "compatible_api_key",
        "COMPATIBLE_API_KEY / saved compatible API key missing — set the env var or save a key under Configuration.",
    )
}

pub fn resolve_gemini_key() -> Result<String> {
    resolve_env_then_keychain(
        "GEMINI_API_KEY",
        "gemini_api_key",
        "GEMINI_API_KEY / saved Gemini key missing — set the env var or save a key under Configuration.",
    )
}
