pub mod api;
pub mod folder_filter;

use crate::config::Config;
use crate::hosts::Host;
use eyre::Result;

pub fn derive_base_url(config: &Config, host: &Host) -> Result<String> {
    if let Some(per_host) = config.bichon_host_base_url(&host.name) {
        return Ok(per_host);
    }
    if let Some(base_url) = config
        .get("bichon_base_url")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    {
        return Ok(base_url.trim_end_matches('/').to_string());
    }
    let domain = config.domain();
    if domain.is_empty() {
        eyre::bail!(
            "no Bichon base URL configured for host '{}'. Set [bichon.hosts.\"{}\"] base_url, or set bichon_base_url, or set domain in config.toml",
            host.name,
            host.name
        )
    }
    let subdomain = config
        .get("bichon_subdomain")
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "bichon".to_string());
    Ok(format!("https://{subdomain}.{domain}"))
}
