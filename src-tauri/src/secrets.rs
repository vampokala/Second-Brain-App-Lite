//! API keys via OS keychain (keyring). Env vars override when present.

use anyhow::Result;
use keyring::Entry;

const SERVICE: &str = "SecondBrainLite";

pub fn set_secret(account: &str, secret: &str) -> Result<()> {
    let entry = Entry::new(SERVICE, account)?;
    entry.set_password(secret)?;
    Ok(())
}

pub fn get_secret(account: &str) -> Result<Option<String>> {
    let entry = Entry::new(SERVICE, account)?;
    match entry.get_password() {
        Ok(s) if !s.is_empty() => Ok(Some(s)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn masked_hint(account: &str) -> Result<Option<String>> {
    Ok(get_secret(account)?.map(|s| {
        if s.len() <= 4 {
            "****".into()
        } else {
            format!("…{}", &s[s.len().saturating_sub(4)..])
        }
    }))
}

pub fn openai_key() -> Option<String> {
    std::env::var("OPENAI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| get_secret("openai_api_key").ok().flatten())
}

pub fn anthropic_key() -> Option<String> {
    std::env::var("ANTHROPIC_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| get_secret("anthropic_api_key").ok().flatten())
}

pub fn compatible_key() -> Option<String> {
    std::env::var("COMPATIBLE_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| get_secret("compatible_api_key").ok().flatten())
}
