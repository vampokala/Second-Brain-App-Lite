//! Brave Web Search + bounded page fetch for chat augmentation.

use crate::config::AppConfig;
use crate::ingest::fetch_url_plain_excerpt;
use crate::secrets::resolve_brave_search_key;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAugmentResult {
    pub context_block: String,
    pub pages_fetched: u32,
}

/// Reject URLs that are not safe to fetch from untrusted search results (SSRF mitigation).
pub fn is_public_http_url_for_fetch(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    if u.scheme() != "http" && u.scheme() != "https" {
        return false;
    }
    let Some(host) = u.host_str() else {
        return false;
    };
    if host.len() > 256 {
        return false;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return is_public_ip(ip);
    }
    true
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            !(v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.octets()[0] == 0)
        }
        std::net::IpAddr::V6(v6) => {
            !(v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local())
        }
    }
}

#[derive(Debug, Deserialize)]
struct BraveEnvelope {
    web: Option<BraveWeb>,
}

#[derive(Debug, Deserialize)]
struct BraveWeb {
    results: Option<Vec<BraveHit>>,
}

#[derive(Debug, Deserialize)]
struct BraveHit {
    url: Option<String>,
    #[allow(dead_code)]
    title: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
}

pub async fn brave_search_and_fetch_context(cfg: &AppConfig, query: &str) -> Result<WebAugmentResult> {
    let api_key = resolve_brave_search_key().context("resolve Brave Search API key")?;
    let count = cfg.brave_search_count.clamp(1, 20);
    let max_urls = cfg.brave_fetch_max_urls.max(1).min(10);
    let timeout = std::time::Duration::from_secs(cfg.brave_fetch_timeout_secs.max(5).min(60));
    let page_chars = cfg.brave_page_max_chars.max(500).min(50_000);
    let max_body = cfg.brave_max_body_bytes.max(10_000).min(5_000_000);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(25))
        .build()?;

    let res = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .query(&[("q", query), ("count", &count.to_string())])
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key.trim())
        .send()
        .await
        .context("Brave search request")?;

    if !res.status().is_success() {
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        anyhow::bail!("Brave search failed: {} — {}", status, body.chars().take(200).collect::<String>());
    }

    let env: BraveEnvelope = res.json().await.context("parse Brave JSON")?;
    let hits = env
        .web
        .and_then(|w| w.results)
        .unwrap_or_default();

    let mut urls: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for h in hits {
        if let Some(u) = h.url.filter(|s| !s.is_empty()) {
            if seen.insert(u.clone()) {
                urls.push(u);
            }
        }
        if urls.len() >= max_urls as usize {
            break;
        }
    }

    if urls.is_empty() {
        anyhow::bail!("Brave search returned no result URLs to fetch.");
    }

    let mut blocks = String::new();
    let mut pages_fetched: u32 = 0;

    for u in urls {
        if !is_public_http_url_for_fetch(&u) {
            continue;
        }
        match fetch_url_plain_excerpt(&u, timeout, max_body, page_chars).await {
            Ok((title, text)) => {
                pages_fetched += 1;
                blocks.push_str(&format!(
                    "#### {}\nURL: {}\n\n{}\n\n---\n\n",
                    title,
                    u,
                    text.trim()
                ));
            }
            Err(_) => continue,
        }
    }

    if blocks.is_empty() {
        anyhow::bail!("Could not fetch any web pages from Brave results (blocked, paywalled, or network errors).");
    }

    Ok(WebAugmentResult {
        context_block: blocks,
        pages_fetched,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_localhost_and_private_v4() {
        assert!(!is_public_http_url_for_fetch("http://localhost/foo"));
        assert!(!is_public_http_url_for_fetch("https://127.0.0.1/"));
        assert!(!is_public_http_url_for_fetch("http://192.168.1.1/"));
        assert!(!is_public_http_url_for_fetch("http://10.0.0.1/"));
        assert!(is_public_http_url_for_fetch("https://example.com/path"));
    }
}
