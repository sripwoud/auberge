use crate::ansible_assets::AnsibleAssets;
use crate::output::{self, OutputFormat};
use crate::playbook_meta::declared_app_versions;
use clap::Args;
use eyre::Result;
use serde::Serialize;
use tabled::Tabled;

#[derive(Args)]
pub struct VersionsCmd {
    #[arg(
        short = 'o',
        long,
        value_enum,
        default_value = "human",
        help = "Output format"
    )]
    pub output: OutputFormat,
}

#[derive(Debug, Serialize)]
struct AppReport {
    app: String,
    declared: String,
}

/// Report the App Version each Playbook Meta declares (ADR-0017). Reads only
/// the embedded asset tree — what the repo declares, not what any Host runs.
pub fn run_versions(cmd: VersionsCmd) -> Result<()> {
    let assets = AnsibleAssets::prepare()?;
    let reports: Vec<AppReport> = declared_app_versions(&assets.playbooks_dir())?
        .into_iter()
        .map(|(app, version)| AppReport {
            app,
            declared: version.value,
        })
        .collect();

    match cmd.output {
        OutputFormat::Human => print_declared_table(&reports),
        OutputFormat::Json => println!("{}", render_json(&reports)?),
    }

    Ok(())
}

#[derive(Tabled)]
struct DeclaredRow<'a> {
    #[tabled(rename = "APP")]
    app: &'a str,
    #[tabled(rename = "DECLARED")]
    declared: &'a str,
}

fn print_declared_table(reports: &[AppReport]) {
    let rows: Vec<DeclaredRow> = reports
        .iter()
        .map(|report| DeclaredRow {
            app: &report.app,
            declared: &report.declared,
        })
        .collect();
    output::print_table(&rows);
}

fn render_json(reports: &[AppReport]) -> Result<String> {
    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "apps": reports,
    }))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_json_lists_each_app_with_its_declared_version() {
        let reports = vec![
            AppReport {
                app: "actual".to_string(),
                declared: "26.8.0".to_string(),
            },
            AppReport {
                app: "headscale".to_string(),
                declared: "0.25.1".to_string(),
            },
        ];

        let json: serde_json::Value =
            serde_json::from_str(&render_json(&reports).unwrap()).unwrap();

        let apps = json["apps"].as_array().unwrap();
        assert_eq!(apps.len(), 2);
        assert_eq!(apps[0]["app"], "actual");
        assert_eq!(apps[0]["declared"], "26.8.0");
        assert_eq!(apps[1]["app"], "headscale");
        assert_eq!(apps[1]["declared"], "0.25.1");
    }
}
