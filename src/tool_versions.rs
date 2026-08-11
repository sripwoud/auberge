use crate::playbook_meta::VersionPin;
use eyre::{Result, WrapErr};
use std::path::Path;

const ANNOTATION: &str = "# renovate: ";

/// A Tool Version: a build or runtime input pinned in a role's
/// `defaults/main.yml` behind a `# renovate:` annotation (ADR-0017, amended
/// for #451). `tool` is the variable name stripped of the role prefix and
/// the `_version` suffix, so `blocky_lego_version` reports as blocky/lego.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolVersion {
    pub role: String,
    pub tool: String,
    pub pin: VersionPin,
}

/// Collect every Tool Version pinned under `<roles_dir>/*/defaults/main.yml`,
/// sorted by (role, tool). The annotation is the same line Renovate's custom
/// manager matches, and `tests/version_annotations.rs` asserts every
/// `_version:` in defaults carries one — so this scan is complete by CI
/// guarantee, not by convention.
pub fn declared_tool_versions(roles_dir: &Path) -> Result<Vec<ToolVersion>> {
    let entries = std::fs::read_dir(roles_dir)
        .wrap_err_with(|| format!("Failed to read roles directory {}", roles_dir.display()))?;

    let mut tools = Vec::new();
    for entry in entries {
        let role_dir = entry
            .wrap_err("Failed to read roles directory entry")?
            .path();
        let Some(role) = role_dir.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let defaults = role_dir.join("defaults/main.yml");
        let Ok(content) = std::fs::read_to_string(&defaults) else {
            continue;
        };
        tools.extend(
            scan_defaults(role, &content).wrap_err_with(|| format!("in {}", defaults.display()))?,
        );
    }
    tools.sort_by(|a, b| (&a.role, &a.tool).cmp(&(&b.role, &b.tool)));
    Ok(tools)
}

fn scan_defaults(role: &str, content: &str) -> Result<Vec<ToolVersion>> {
    let mut tools = Vec::new();
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        let Some(annotation) = line.strip_prefix(ANNOTATION) else {
            continue;
        };
        let pinned = lines
            .next()
            .filter(|next| !next.trim().is_empty())
            .ok_or_else(|| eyre::eyre!("`{line}` is not followed by a pinned variable"))?;
        tools.push(tool_version(role, annotation, pinned)?);
    }
    Ok(tools)
}

fn tool_version(role: &str, annotation: &str, pinned: &str) -> Result<ToolVersion> {
    let (variable, value) = pinned
        .split_once(':')
        .ok_or_else(|| eyre::eyre!("`{pinned}` is not a `variable: value` line"))?;
    let tool = variable
        .strip_prefix(role)
        .and_then(|rest| rest.strip_prefix('_'))
        .and_then(|rest| rest.strip_suffix("_version"))
        .filter(|tool| !tool.is_empty())
        .ok_or_else(|| eyre::eyre!("`{variable}` does not follow `<role>_<tool>_version`"))?;
    Ok(ToolVersion {
        role: role.to_string(),
        tool: tool.to_string(),
        pin: pin(annotation, value.trim().trim_matches('"'))
            .wrap_err_with(|| format!("malformed annotation for `{variable}`"))?,
    })
}

/// Parse the `key=value` coordinates of a `# renovate:` annotation — the
/// vocabulary of Renovate's regex manager, same as a Playbook Meta
/// `version:` block.
fn pin(annotation: &str, value: &str) -> Result<VersionPin> {
    let mut datasource = None;
    let mut dep_name = None;
    let mut versioning = None;
    let mut extract_version = None;
    for token in annotation.split_whitespace() {
        let (key, coordinate) = token
            .split_once('=')
            .ok_or_else(|| eyre::eyre!("`{token}` is not a `key=value` coordinate"))?;
        let coordinate = Some(coordinate.to_string());
        match key {
            "datasource" => datasource = coordinate,
            "depName" => dep_name = coordinate,
            "versioning" => versioning = coordinate,
            "extractVersion" => extract_version = coordinate,
            other => eyre::bail!("unknown coordinate `{other}`"),
        }
    }
    Ok(VersionPin {
        value: value.to_string(),
        datasource: datasource.ok_or_else(|| eyre::eyre!("missing `datasource`"))?,
        dep_name: dep_name.ok_or_else(|| eyre::eyre!("missing `depName`"))?,
        versioning,
        extract_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roles_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ansible/roles")
    }

    #[test]
    fn test_scan_defaults_reads_coordinates_and_strips_quotes() {
        let content = "---\n\
            # renovate: datasource=github-releases depName=go-acme/lego extractVersion=^v(?<version>.+)$\n\
            blocky_lego_version: \"5.3.1\"\n\
            blocky_lego_url: \"https://example.com\"\n";

        let tools = scan_defaults("blocky", content).unwrap();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].role, "blocky");
        assert_eq!(tools[0].tool, "lego");
        assert_eq!(tools[0].pin.value, "5.3.1");
        assert_eq!(tools[0].pin.datasource, "github-releases");
        assert_eq!(tools[0].pin.dep_name, "go-acme/lego");
        assert_eq!(
            tools[0].pin.extract_version.as_deref(),
            Some("^v(?<version>.+)$")
        );
        assert!(tools[0].pin.versioning.is_none());
    }

    #[test]
    fn test_scan_defaults_reads_multiple_annotations_per_file() {
        let content = "# renovate: datasource=go depName=github.com/mholt/caddy-l4\n\
            caddy_l4_version: \"v0.1.2\"\n\
            # renovate: datasource=go depName=github.com/caddy-dns/cloudflare\n\
            caddy_cloudflare_plugin_version: \"v0.2.4\"\n";

        let tools = scan_defaults("caddy", content).unwrap();

        let names: Vec<&str> = tools.iter().map(|t| t.tool.as_str()).collect();
        assert_eq!(names, ["l4", "cloudflare_plugin"]);
        assert_eq!(tools[1].pin.dep_name, "github.com/caddy-dns/cloudflare");
    }

    #[test]
    fn test_dangling_annotation_fails_fast() {
        let content = "# renovate: datasource=go depName=example.com/mod\n";
        assert!(scan_defaults("caddy", content).is_err());
    }

    #[test]
    fn test_variable_outside_role_naming_convention_fails_fast() {
        let content = "# renovate: datasource=go depName=example.com/mod\n\
            lego_version: \"5.3.1\"\n";
        assert!(scan_defaults("blocky", content).is_err());
    }

    #[test]
    fn test_annotation_missing_dep_name_fails_fast() {
        let content = "# renovate: datasource=go\n\
            caddy_l4_version: \"v0.1.2\"\n";
        assert!(scan_defaults("caddy", content).is_err());
    }

    #[test]
    fn test_declared_tool_versions_matches_the_repo_allowlist() {
        let tools = declared_tool_versions(&roles_dir()).unwrap();

        let variables: Vec<String> = tools
            .iter()
            .map(|t| format!("{}_{}_version", t.role, t.tool))
            .collect();
        assert_eq!(
            variables,
            [
                "blocky_lego_version",
                "caddy_cloudflare_plugin_version",
                "caddy_l4_version",
                "hermes_uv_version",
                "tgtg_uv_version",
            ],
            "Tool Versions diverged from tests/version_annotations.rs TOOL_VERSIONS"
        );
        assert!(tools.iter().all(|t| !t.pin.value.is_empty()));
    }
}
