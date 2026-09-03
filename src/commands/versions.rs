use crate::ansible_assets::AnsibleAssets;
use crate::output::{self, OutputArg, OutputFormat};
use crate::playbook_meta::{VersionPin, declared_app_versions};
use crate::tool_versions::{ToolVersion, declared_tool_versions};
use clap::Args;
use eyre::{Result, WrapErr};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use tabled::Tabled;

const NPM_REGISTRY: &str = "https://registry.npmjs.org";
const GITHUB_API: &str = "https://api.github.com";
const GO_PROXY: &str = "https://proxy.golang.org";
// Abbreviated packument: dist-tags without per-version metadata (~1% the size).
const NPM_ABBREVIATED: &str = "application/vnd.npm.install-v1+json";

#[derive(Args)]
pub struct VersionsCmd {
    #[arg(
        long,
        help = "Query each App's datasource for its latest release and report drift"
    )]
    pub check_upstream: bool,
    #[command(flatten)]
    pub output: OutputArg,
}

#[derive(Debug, Serialize)]
struct AppReport {
    app: String,
    declared: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<DriftStatus>,
}

#[derive(Debug, Serialize)]
struct ToolReport {
    role: String,
    tool: String,
    declared: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<DriftStatus>,
}

/// App and Tool Versions stay distinct sections end to end — the ADR-0017
/// split is visible in the output rather than flattened into one list.
#[derive(Debug, Serialize)]
struct VersionsReport {
    apps: Vec<AppReport>,
    tools: Vec<ToolReport>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum DriftStatus {
    Current,
    Behind,
    Unknown,
}

impl DriftStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Behind => "behind",
            Self::Unknown => "unknown",
        }
    }
}

/// Returns the process exit code — the Backup Verdict convention (0 every
/// pin current, 1 at least one behind, 2 operational error) so a cron can
/// branch on drift. `unknown` drift does not fail the gate.
pub async fn run_versions(cmd: VersionsCmd) -> i32 {
    versions_exit_code(versions_and_report(cmd).await)
}

fn versions_exit_code(result: Result<VersionsReport>) -> i32 {
    match result {
        Ok(report) => {
            let behind = report
                .apps
                .iter()
                .map(|app| app.status)
                .chain(report.tools.iter().map(|tool| tool.status))
                .any(|status| status == Some(DriftStatus::Behind));
            i32::from(behind)
        }
        Err(e) => {
            eprintln!("✗ {e:#}");
            2
        }
    }
}

/// Report the App Version each Playbook Meta declares and the Tool Versions
/// annotated in role defaults (ADR-0017, amended for #451). Reads only the
/// embedded asset tree — what the repo declares, not what any Host runs.
/// Upstream is queried only behind `--check-upstream`; the default path stays
/// offline.
async fn versions_and_report(cmd: VersionsCmd) -> Result<VersionsReport> {
    let assets = AnsibleAssets::prepare()?;
    let apps = declared_app_versions(&assets.playbooks_dir())?;
    let tools = declared_tool_versions(&assets.roles_dir())?;

    let report = if cmd.check_upstream {
        let client = UpstreamClient::new()?;
        VersionsReport {
            apps: app_drift_reports(&apps, &client).await?,
            tools: tool_drift_reports(&tools, &client).await?,
        }
    } else {
        VersionsReport {
            apps: apps
                .into_iter()
                .map(|(app, pin)| AppReport {
                    app,
                    declared: pin.value,
                    latest: None,
                    status: None,
                })
                .collect(),
            tools: tools
                .into_iter()
                .map(|tool| ToolReport {
                    role: tool.role,
                    tool: tool.tool,
                    declared: tool.pin.value,
                    latest: None,
                    status: None,
                })
                .collect(),
        }
    };

    match cmd.output.format {
        OutputFormat::Human => print_report_tables(&report, cmd.check_upstream),
        OutputFormat::Json => println!("{}", render_json(&report, cmd.check_upstream)?),
    }

    Ok(report)
}

async fn resolve_drift(
    client: &UpstreamClient,
    name: &str,
    pin: &VersionPin,
) -> Result<(String, DriftStatus)> {
    let latest = client
        .latest(pin)
        .await
        .wrap_err_with(|| format!("Failed to resolve the latest {name} release"))?;
    let status = drift(&pin.value, &latest);
    Ok((latest, status))
}

async fn app_drift_reports(
    declared: &[(String, VersionPin)],
    client: &UpstreamClient,
) -> Result<Vec<AppReport>> {
    let mut reports = Vec::with_capacity(declared.len());
    for (app, pin) in declared {
        let (latest, status) = resolve_drift(client, app, pin).await?;
        reports.push(AppReport {
            app: app.clone(),
            declared: pin.value.clone(),
            latest: Some(latest),
            status: Some(status),
        });
    }
    Ok(reports)
}

async fn tool_drift_reports(
    declared: &[ToolVersion],
    client: &UpstreamClient,
) -> Result<Vec<ToolReport>> {
    let mut reports = Vec::with_capacity(declared.len());
    for tool in declared {
        let name = format!("{}/{}", tool.role, tool.tool);
        let (latest, status) = resolve_drift(client, &name, &tool.pin).await?;
        reports.push(ToolReport {
            role: tool.role.clone(),
            tool: tool.tool.clone(),
            declared: tool.pin.value.clone(),
            latest: Some(latest),
            status: Some(status),
        });
    }
    Ok(reports)
}

/// Pure drift comparison over (declared, latest). `Unknown` when either side
/// is not version-shaped — mirroring Renovate, which skips values its
/// versioning cannot parse.
fn drift(declared: &str, latest: &str) -> DriftStatus {
    if !is_version_like(declared) || !is_version_like(latest) {
        return DriftStatus::Unknown;
    }
    match compare_versions(declared, latest) {
        Ordering::Less => DriftStatus::Behind,
        Ordering::Equal | Ordering::Greater => DriftStatus::Current,
    }
}

/// Version-shaped: a leading numeric segment after an optional `v` prefix.
fn is_version_like(version: &str) -> bool {
    segments(version)
        .first()
        .is_some_and(|segment| segment.parse::<u64>().is_ok())
}

fn segments(version: &str) -> Vec<&str> {
    version
        .strip_prefix('v')
        .unwrap_or(version)
        .split('.')
        .collect()
}

/// Total order over version strings: numeric segments compare numerically
/// (2.9 < 2.10), a missing segment counts as 0 (1.2 == 1.2.0), non-numeric
/// segments fall back to lexical comparison.
fn compare_versions(a: &str, b: &str) -> Ordering {
    let a = segments(a);
    let b = segments(b);
    for i in 0..a.len().max(b.len()) {
        let left = a.get(i).copied().unwrap_or("0");
        let right = b.get(i).copied().unwrap_or("0");
        let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(l), Ok(r)) => l.cmp(&r),
            _ => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    Ordering::Equal
}

struct UpstreamClient {
    http: reqwest::Client,
    npm_base: String,
    github_base: String,
    go_base: String,
}

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
}

impl UpstreamClient {
    fn new() -> Result<Self> {
        Self::with_bases(
            NPM_REGISTRY.to_string(),
            GITHUB_API.to_string(),
            GO_PROXY.to_string(),
        )
    }

    fn with_bases(npm_base: String, github_base: String, go_base: String) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("auberge/", env!("CARGO_PKG_VERSION")))
            .build()
            .wrap_err("Failed to build HTTP client")?;
        Ok(Self {
            http,
            npm_base,
            github_base,
            go_base,
        })
    }

    async fn latest(&self, version: &VersionPin) -> Result<String> {
        match version.datasource.as_str() {
            "npm" => self.npm_latest(&version.dep_name).await,
            "github-releases" => self.github_latest(version).await,
            "go" => self.go_latest(&version.dep_name).await,
            other => eyre::bail!("Unsupported datasource `{other}`"),
        }
    }

    /// Renovate's `go` datasource resolves through the module proxy:
    /// `@v/list` yields every known tagged version, one per line. Stability
    /// and ordering reuse the shared rules, so a `go` pin drifts exactly like
    /// a release tag.
    async fn go_latest(&self, dep_name: &str) -> Result<String> {
        let url = format!("{}/{}/@v/list", self.go_base, escape_go_module(dep_name));
        let listing = self
            .http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        listing
            .lines()
            .map(str::trim)
            .filter(|version| is_version_like(version) && is_stable(version))
            .max_by(|a, b| compare_versions(a, b))
            .map(str::to_string)
            .ok_or_else(|| eyre::eyre!("the module proxy lists no stable version of {dep_name}"))
    }

    async fn npm_latest(&self, dep_name: &str) -> Result<String> {
        let url = format!("{}/{}", self.npm_base, dep_name);
        let packument: serde_json::Value = self
            .http
            .get(&url)
            .header(reqwest::header::ACCEPT, NPM_ABBREVIATED)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        packument["dist-tags"]["latest"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| eyre::eyre!("npm packument for {dep_name} has no dist-tags.latest"))
    }

    async fn github_latest(&self, version: &VersionPin) -> Result<String> {
        let url = format!(
            "{}/repos/{}/releases?per_page=100",
            self.github_base, version.dep_name
        );
        let mut request = self
            .http
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/vnd.github+json");
        if let Ok(token) = std::env::var("GITHUB_TOKEN") {
            request = request.bearer_auth(token);
        }
        let releases: Vec<Release> = request.send().await?.error_for_status()?.json().await?;
        let extract = version
            .extract_version
            .as_deref()
            .map(Regex::new)
            .transpose()
            .wrap_err_with(|| format!("Invalid extractVersion for {}", version.dep_name))?;
        latest_release_version(&releases, extract.as_ref()).ok_or_else(|| {
            eyre::eyre!(
                "no release of {} yields a version through its coordinates",
                version.dep_name
            )
        })
    }
}

/// Renovate's github-releases semantics: the newest stable release whose tag
/// yields a version — verbatim, or through extractVersion's `version` capture,
/// which also filters monorepo tags (grimmory's `grimmory/vX.Y.Z` among
/// auberge's own releases).
fn latest_release_version(releases: &[Release], extract: Option<&Regex>) -> Option<String> {
    releases
        .iter()
        .filter(|release| !release.draft && !release.prerelease)
        .filter_map(|release| tag_version(&release.tag_name, extract))
        .filter(|version| is_version_like(version) && is_stable(version))
        .max_by(|a, b| compare_versions(a, b))
}

/// Renovate's default stability rule: a hyphenated suffix (2.0.1-alpha.1)
/// marks a prerelease even when the GitHub release is not flagged as one —
/// bichon publishes exactly such tags.
fn is_stable(version: &str) -> bool {
    !version.contains('-')
}

/// Module paths are case-encoded on the proxy: each uppercase letter becomes
/// `!` plus its lowercase form (golang.org/x/mod's escaped-path rule).
fn escape_go_module(module: &str) -> String {
    let mut escaped = String::with_capacity(module.len());
    for c in module.chars() {
        if c.is_ascii_uppercase() {
            escaped.push('!');
            escaped.push(c.to_ascii_lowercase());
        } else {
            escaped.push(c);
        }
    }
    escaped
}

fn tag_version(tag: &str, extract: Option<&Regex>) -> Option<String> {
    match extract {
        None => Some(tag.to_string()),
        Some(regex) => regex
            .captures(tag)
            .and_then(|captures| captures.name("version"))
            .map(|capture| capture.as_str().to_string()),
    }
}

#[derive(Tabled)]
struct DeclaredAppRow<'a> {
    #[tabled(rename = "APP")]
    app: &'a str,
    #[tabled(rename = "DECLARED")]
    declared: &'a str,
}

#[derive(Tabled)]
struct DriftAppRow<'a> {
    #[tabled(rename = "APP")]
    app: &'a str,
    #[tabled(rename = "DECLARED")]
    declared: &'a str,
    #[tabled(rename = "LATEST")]
    latest: &'a str,
    #[tabled(rename = "STATUS")]
    status: &'static str,
}

#[derive(Tabled)]
struct DeclaredToolRow<'a> {
    #[tabled(rename = "ROLE")]
    role: &'a str,
    #[tabled(rename = "TOOL")]
    tool: &'a str,
    #[tabled(rename = "DECLARED")]
    declared: &'a str,
}

#[derive(Tabled)]
struct DriftToolRow<'a> {
    #[tabled(rename = "ROLE")]
    role: &'a str,
    #[tabled(rename = "TOOL")]
    tool: &'a str,
    #[tabled(rename = "DECLARED")]
    declared: &'a str,
    #[tabled(rename = "LATEST")]
    latest: &'a str,
    #[tabled(rename = "STATUS")]
    status: &'static str,
}

fn print_report_tables(report: &VersionsReport, checked_upstream: bool) {
    println!("App Versions");
    if checked_upstream {
        let rows: Vec<DriftAppRow> = report
            .apps
            .iter()
            .map(|app| DriftAppRow {
                app: &app.app,
                declared: &app.declared,
                latest: app.latest.as_deref().expect("drift reports carry latest"),
                status: app.status.expect("drift reports carry status").as_str(),
            })
            .collect();
        output::print_table(&rows);
    } else {
        let rows: Vec<DeclaredAppRow> = report
            .apps
            .iter()
            .map(|app| DeclaredAppRow {
                app: &app.app,
                declared: &app.declared,
            })
            .collect();
        output::print_table(&rows);
    }

    println!("\nTool Versions");
    if checked_upstream {
        let rows: Vec<DriftToolRow> = report
            .tools
            .iter()
            .map(|tool| DriftToolRow {
                role: &tool.role,
                tool: &tool.tool,
                declared: &tool.declared,
                latest: tool.latest.as_deref().expect("drift reports carry latest"),
                status: tool.status.expect("drift reports carry status").as_str(),
            })
            .collect();
        output::print_table(&rows);
    } else {
        let rows: Vec<DeclaredToolRow> = report
            .tools
            .iter()
            .map(|tool| DeclaredToolRow {
                role: &tool.role,
                tool: &tool.tool,
                declared: &tool.declared,
            })
            .collect();
        output::print_table(&rows);
    }
}

fn render_json(report: &VersionsReport, checked_upstream: bool) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "checked_upstream": checked_upstream,
        "apps": report.apps,
        "tools": report.tools,
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn report(app: &str, declared: &str, latest: Option<&str>) -> AppReport {
        AppReport {
            app: app.to_string(),
            declared: declared.to_string(),
            status: latest.map(|latest| drift(declared, latest)),
            latest: latest.map(str::to_string),
        }
    }

    fn tool_report(role: &str, tool: &str, declared: &str, latest: Option<&str>) -> ToolReport {
        ToolReport {
            role: role.to_string(),
            tool: tool.to_string(),
            declared: declared.to_string(),
            status: latest.map(|latest| drift(declared, latest)),
            latest: latest.map(str::to_string),
        }
    }

    fn versions_report(apps: Vec<AppReport>, tools: Vec<ToolReport>) -> VersionsReport {
        VersionsReport { apps, tools }
    }

    fn pin(datasource: &str, dep_name: &str, extract_version: Option<&str>) -> VersionPin {
        VersionPin {
            value: "1.0.0".to_string(),
            datasource: datasource.to_string(),
            dep_name: dep_name.to_string(),
            versioning: None,
            extract_version: extract_version.map(str::to_string),
        }
    }

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.to_string(),
            draft: false,
            prerelease: false,
        }
    }

    #[test]
    fn drift_is_a_pure_table_over_declared_and_latest() {
        let cases = [
            ("26.8.1", "26.8.1", DriftStatus::Current),
            ("26.8.0", "26.8.1", DriftStatus::Behind),
            ("2.20.10", "2.21.0", DriftStatus::Behind),
            ("2.9.3", "2.10.0", DriftStatus::Behind), // numeric, not lexical
            ("0.29.3", "0.25.1", DriftStatus::Current), // ahead is not behind
            ("1.2", "1.2.0", DriftStatus::Current),
            ("v2026.4.13", "v2026.4.20", DriftStatus::Behind), // hermes keeps its v prefix
            ("2.2.4", "v2.3.0", DriftStatus::Behind),          // raw v-tag as latest
            ("edge", "1.26.2", DriftStatus::Unknown),          // not version-shaped
            ("1.0.0", "nightly", DriftStatus::Unknown),
        ];
        for (declared, latest, expected) in cases {
            assert_eq!(drift(declared, latest), expected, "({declared}, {latest})");
        }
    }

    #[test]
    fn latest_release_version_picks_the_numeric_max() {
        let releases = [release("v0.9.0"), release("v0.10.0"), release("v0.2.0")];
        assert_eq!(latest_release_version(&releases, None).unwrap(), "v0.10.0");
    }

    #[test]
    fn latest_release_version_skips_drafts_and_prereleases() {
        let releases = [
            Release {
                tag_name: "v2.0.0".to_string(),
                draft: true,
                prerelease: false,
            },
            Release {
                tag_name: "v1.9.0".to_string(),
                draft: false,
                prerelease: true,
            },
            release("v1.8.0"),
        ];
        assert_eq!(latest_release_version(&releases, None).unwrap(), "v1.8.0");
    }

    #[test]
    fn extract_version_filters_monorepo_tags_and_strips_the_prefix() {
        let releases = [
            release("v0.14.12"),
            release("grimmory/v2.3.0"),
            release("grimmory/v2.4.0"),
        ];
        let extract = Regex::new("^grimmory/v(?<version>.+)$").unwrap();
        assert_eq!(
            latest_release_version(&releases, Some(&extract)).unwrap(),
            "2.4.0"
        );
    }

    #[test]
    fn latest_release_version_skips_unflagged_hyphenated_prereleases() {
        let releases = [release("2.0.1-alpha.1"), release("2.0.0")];
        assert_eq!(latest_release_version(&releases, None).unwrap(), "2.0.0");
    }

    #[test]
    fn latest_release_version_is_none_when_no_tag_is_version_shaped() {
        let releases = [release("nightly")];
        assert_eq!(latest_release_version(&releases, None), None);
    }

    #[test]
    fn render_json_offline_omits_latest_and_status() {
        let report = versions_report(
            vec![report("actual", "26.8.0", None)],
            vec![tool_report("blocky", "lego", "5.3.1", None)],
        );

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report, false).unwrap()).unwrap();

        assert_eq!(json["checked_upstream"], false);
        let app = &json["apps"][0];
        assert_eq!(app["app"], "actual");
        assert_eq!(app["declared"], "26.8.0");
        assert!(app.get("latest").is_none());
        assert!(app.get("status").is_none());
        let tool = &json["tools"][0];
        assert_eq!(tool["role"], "blocky");
        assert_eq!(tool["tool"], "lego");
        assert_eq!(tool["declared"], "5.3.1");
        assert!(tool.get("latest").is_none());
        assert!(tool.get("status").is_none());
    }

    #[test]
    fn render_json_with_drift_reports_latest_and_status() {
        let report = versions_report(
            vec![
                report("actual", "26.8.0", Some("26.8.1")),
                report("headscale", "0.29.3", Some("0.29.3")),
            ],
            vec![tool_report("caddy", "l4", "v0.1.2", Some("v0.2.0"))],
        );

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&report, true).unwrap()).unwrap();

        assert_eq!(json["checked_upstream"], true);
        assert_eq!(json["apps"][0]["latest"], "26.8.1");
        assert_eq!(json["apps"][0]["status"], "behind");
        assert_eq!(json["apps"][1]["status"], "current");
        assert_eq!(json["tools"][0]["latest"], "v0.2.0");
        assert_eq!(json["tools"][0]["status"], "behind");
    }

    #[test]
    fn versions_exit_code_mirrors_the_backup_verdict_convention() {
        let current = || report("a", "1.0.0", Some("1.0.0"));
        let behind = || report("b", "1.0.0", Some("2.0.0"));
        let unknown = || report("c", "edge", Some("1.0.0"));
        let offline = || report("d", "1.0.0", None);

        let ok = |apps, tools| Ok(versions_report(apps, tools));

        assert_eq!(
            versions_exit_code(ok(vec![current(), unknown()], vec![])),
            0
        );
        assert_eq!(versions_exit_code(ok(vec![current(), behind()], vec![])), 1);
        assert_eq!(versions_exit_code(ok(vec![offline()], vec![])), 0);
        assert_eq!(versions_exit_code(Err(eyre::eyre!("boom"))), 2);
    }

    #[test]
    fn versions_exit_code_fails_the_gate_on_tool_drift_alone() {
        let report = versions_report(
            vec![report("a", "1.0.0", Some("1.0.0"))],
            vec![tool_report("blocky", "lego", "4.24.0", Some("5.3.1"))],
        );

        assert_eq!(versions_exit_code(Ok(report)), 1);
    }

    #[tokio::test]
    async fn npm_latest_reads_dist_tags_from_the_registry() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/@actual-app/sync-server"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "dist-tags": { "latest": "26.8.1" }
            })))
            .mount(&server)
            .await;
        let client = UpstreamClient::with_bases(server.uri(), server.uri(), server.uri())?;

        let latest = client
            .latest(&pin("npm", "@actual-app/sync-server", None))
            .await?;

        assert_eq!(latest, "26.8.1");
        Ok(())
    }

    #[tokio::test]
    async fn github_latest_extracts_versions_from_release_tags() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/sripwoud/auberge/releases"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                { "tag_name": "v0.14.12" },
                { "tag_name": "grimmory/v2.4.0" },
                { "tag_name": "grimmory/v2.3.0" },
            ])))
            .mount(&server)
            .await;
        let client = UpstreamClient::with_bases(server.uri(), server.uri(), server.uri())?;

        let latest = client
            .latest(&pin(
                "github-releases",
                "sripwoud/auberge",
                Some("^grimmory/v(?<version>.+)$"),
            ))
            .await?;

        assert_eq!(latest, "2.4.0");
        Ok(())
    }

    #[tokio::test]
    async fn go_latest_picks_the_max_stable_version_from_the_module_proxy() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/github.com/mholt/caddy-l4/@v/list"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("v0.1.0\nv0.1.2\nv0.2.0-beta.1\n"),
            )
            .mount(&server)
            .await;
        let client = UpstreamClient::with_bases(server.uri(), server.uri(), server.uri())?;

        let latest = client
            .latest(&pin("go", "github.com/mholt/caddy-l4", None))
            .await?;

        assert_eq!(latest, "v0.1.2");
        Ok(())
    }

    #[tokio::test]
    async fn go_latest_fails_when_the_proxy_lists_no_stable_version() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("v0.2.0-beta.1\n"))
            .mount(&server)
            .await;
        let client = UpstreamClient::with_bases(server.uri(), server.uri(), server.uri())?;

        let result = client.latest(&pin("go", "example.com/mod", None)).await;

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("no stable version")
        );
        Ok(())
    }

    #[test]
    fn escape_go_module_encodes_uppercase_as_bang_lowercase() {
        assert_eq!(
            escape_go_module("github.com/Azure/azure-sdk-for-go"),
            "github.com/!azure/azure-sdk-for-go"
        );
        assert_eq!(
            escape_go_module("github.com/mholt/caddy-l4"),
            "github.com/mholt/caddy-l4"
        );
    }

    #[tokio::test]
    async fn latest_fails_fast_on_upstream_http_errors() -> Result<()> {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let client = UpstreamClient::with_bases(server.uri(), server.uri(), server.uri())?;

        let result = client
            .latest(&pin("github-releases", "juanfont/headscale", None))
            .await;

        assert!(result.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn latest_rejects_an_unsupported_datasource() -> Result<()> {
        let client = UpstreamClient::with_bases(
            "http://unused".to_string(),
            "http://unused".to_string(),
            "http://unused".to_string(),
        )?;

        let result = client.latest(&pin("docker", "some/image", None)).await;

        assert!(result.unwrap_err().to_string().contains("docker"));
        Ok(())
    }
}
