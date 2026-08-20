use crate::config::Preflight;
use crate::output;
use crate::services::progress::Progress;
use eyre::{Result, WrapErr};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::process::Command;

const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

fn parse_ansible_task(line: &str) -> Option<String> {
    let rest = line.trim().strip_prefix("TASK [")?;
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

fn format_ansible_task(task: &str) -> String {
    if let Some((role, name)) = task.split_once(" : ") {
        if std::io::IsTerminal::is_terminal(&std::io::stderr()) {
            format!("{DIM}{}:{RESET} {}", role, name)
        } else {
            format!("{}: {}", role, name)
        }
    } else {
        task.to_string()
    }
}

pub struct AnsibleResult {
    pub success: bool,
    pub exit_code: i32,
    pub last_output: String,
}

const MAX_FAILURE_LINES: usize = 10;
const MAX_RECAP_LINES: usize = 15;

/// Harvests failure diagnostics from ansible's stdout stream.
///
/// Ansible's default callback writes task failures (`fatal:`/`failed:`) and
/// the PLAY RECAP to stdout; stderr carries only warnings and pre-play
/// `ERROR!` messages. The stderr tail alone therefore misses the root cause
/// of any task failure (#542).
///
/// An ignored task prints `...ignoring` directly after its failure lines
/// (per-item `failed:` lines for loops, with no intervening output), so the
/// whole trailing run of failures is cancelled. Failures of distinct tasks
/// are always separated by a `TASK [...]` header, which resets the run.
#[derive(Default)]
struct FailureDigest {
    failures: Vec<String>,
    recap: Vec<String>,
    in_recap: bool,
    trailing_failures: usize,
}

impl FailureDigest {
    fn observe(&mut self, line: &str) {
        let trimmed = line.trim();
        if trimmed.starts_with("PLAY RECAP") {
            self.in_recap = true;
        }
        if self.in_recap {
            if !trimmed.is_empty() && self.recap.len() < MAX_RECAP_LINES {
                self.recap.push(trimmed.to_string());
            }
            return;
        }
        if trimmed == "...ignoring" {
            self.failures
                .truncate(self.failures.len() - self.trailing_failures);
            self.trailing_failures = 0;
            return;
        }
        if trimmed.starts_with("fatal:") || trimmed.starts_with("failed:") {
            if self.failures.len() < MAX_FAILURE_LINES {
                self.failures.push(trimmed.to_string());
                self.trailing_failures += 1;
            }
            return;
        }
        self.trailing_failures = 0;
    }

    fn has_failures(&self) -> bool {
        !self.failures.is_empty()
    }

    fn render(&self) -> String {
        self.failures
            .iter()
            .chain(self.recap.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Env vars override the embedded ansible.cfg, so a user-exported
/// `ANSIBLE_CALLBACK_RESULT_FORMAT=yaml` (or a custom stdout callback) would
/// break the line-based scraping in `FailureDigest` and `parse_ansible_task`.
fn pin_output_format(cmd: &mut Command) {
    cmd.env("ANSIBLE_STDOUT_CALLBACK", "ansible.builtin.default")
        .env("ANSIBLE_CALLBACK_RESULT_FORMAT", "json");
}

pub struct InventoryHost {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub user: String,
    pub groups: Vec<String>,
}

fn extra_var_args(extra_vars: Option<&[(&str, &str)]>) -> Vec<String> {
    extra_vars
        .into_iter()
        .flatten()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

fn write_extra_vars_file(flat_vars: &HashMap<String, String>) -> Result<tempfile::NamedTempFile> {
    let yaml = serde_yaml::to_string(flat_vars).wrap_err("Failed to serialize config to YAML")?;
    let mut tmpfile = tempfile::NamedTempFile::new().wrap_err("Failed to create temp file")?;
    tmpfile
        .write_all(yaml.as_bytes())
        .wrap_err("Failed to write extra-vars file")?;
    Ok(tmpfile)
}

fn write_inventory_file(host: &InventoryHost) -> Result<tempfile::NamedTempFile> {
    use serde_yaml::{Mapping, Value};

    let mut host_vars = Mapping::new();
    host_vars.insert(
        Value::String("ansible_host".into()),
        Value::String(host.address.clone()),
    );
    host_vars.insert(
        Value::String("ansible_port".into()),
        Value::Number(host.port.into()),
    );

    let mut hosts = Mapping::new();
    hosts.insert(Value::String(host.name.clone()), Value::Mapping(host_vars));

    let mut vps = Mapping::new();
    vps.insert(Value::String("hosts".into()), Value::Mapping(hosts));

    let mut children = Mapping::new();
    children.insert(Value::String("vps".into()), Value::Mapping(vps));

    for group in &host.groups {
        if children.contains_key(Value::String(group.clone())) {
            continue;
        }
        let mut group_hosts = Mapping::new();
        group_hosts.insert(Value::String(host.name.clone()), Value::Null);
        let mut group_entry = Mapping::new();
        group_entry.insert(Value::String("hosts".into()), Value::Mapping(group_hosts));
        children.insert(Value::String(group.clone()), Value::Mapping(group_entry));
    }

    let mut all = Mapping::new();
    all.insert(Value::String("children".into()), Value::Mapping(children));

    let mut root = Mapping::new();
    root.insert(Value::String("all".into()), Value::Mapping(all));

    let yaml =
        serde_yaml::to_string(&Value::Mapping(root)).wrap_err("Failed to serialize inventory")?;

    let mut tmpfile = tempfile::NamedTempFile::new().wrap_err("Failed to create temp file")?;
    tmpfile
        .write_all(yaml.as_bytes())
        .wrap_err("Failed to write inventory file")?;
    Ok(tmpfile)
}

#[allow(clippy::too_many_arguments)]
pub fn run_playbook(
    preflight: &Preflight,
    playbook: &Path,
    host: &InventoryHost,
    check: bool,
    tags: Option<&[String]>,
    skip_tags: Option<&[String]>,
    extra_vars: Option<&[(&str, &str)]>,
    ask_vault_pass: bool,
    ask_pass: bool,
    progress: &mut dyn Progress,
) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file(preflight.flat_vars())?;
    let inventory_file = write_inventory_file(host)?;

    let mut cmd = Command::new("ansible-playbook");
    cmd.current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg("-i")
        .arg(inventory_file.path())
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(&host.name)
        .arg("--extra-vars")
        .arg(format!("@{}", vars_file.path().display()));

    let playbook_name = playbook.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let is_fresh_bootstrap = playbook_name == "bootstrap.yml";

    if check {
        cmd.arg("--check");
    }

    if ask_vault_pass {
        cmd.arg("--ask-vault-pass");
    }

    if is_fresh_bootstrap {
        cmd.arg("--ask-pass");
        cmd.arg("-e").arg(format!("ansible_port={}", host.port));
        cmd.arg("-e").arg(format!("ansible_user={}", host.user));
        cmd.arg("-e").arg(
            "ansible_ssh_common_args='-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null'",
        );
    }

    if ask_pass && !is_fresh_bootstrap {
        cmd.arg("--ask-pass");
    }

    if let Some(tags) = tags {
        cmd.arg("--tags").arg(tags.join(","));
    }

    if let Some(skip_tags) = skip_tags {
        cmd.arg("--skip-tags").arg(skip_tags.join(","));
    }

    for var in extra_var_args(extra_vars) {
        cmd.arg("-e").arg(var);
    }

    let needs_tty = ask_vault_pass || ask_pass || is_fresh_bootstrap;
    if needs_tty {
        let status = cmd
            .status()
            .wrap_err("Failed to execute ansible-playbook")?;
        return Ok(AnsibleResult {
            success: status.success(),
            exit_code: status.code().unwrap_or(-1),
            last_output: String::new(),
        });
    }

    let playbook_label = playbook
        .file_stem()
        .and_then(|n| n.to_str())
        .unwrap_or("ansible");
    pin_output_format(&mut cmd);
    progress.task_started(&format!("Running {}...", playbook_label));
    let mut digest = FailureDigest::default();
    let result = output::stream_command_stdout("ansible", &mut cmd, |line| {
        digest.observe(line);
        if let Some(task) = parse_ansible_task(line) {
            progress.task_started(&format!("Running: {}", format_ansible_task(&task)));
        }
    })
    .wrap_err("Failed to execute ansible-playbook")?;
    progress.task_done();

    let success = result.status.success();
    let last_output = if success || !digest.has_failures() {
        result.last_stderr
    } else {
        digest.render()
    };

    Ok(AnsibleResult {
        success,
        exit_code: result.status.code().unwrap_or(-1),
        last_output,
    })
}

pub fn run_bootstrap(
    preflight: &Preflight,
    playbook: &Path,
    host: &InventoryHost,
) -> Result<AnsibleResult> {
    let assets = crate::ansible_assets::AnsibleAssets::prepare()?;
    assets.ensure_collections()?;
    let ansible_dir = assets.ansible_dir().to_path_buf();
    let vars_file = write_extra_vars_file(preflight.flat_vars())?;
    let inventory_file = write_inventory_file(host)?;

    let status = Command::new("ansible-playbook")
        .current_dir(&ansible_dir)
        .arg("-i")
        .arg("inventory.yml")
        .arg("-i")
        .arg(inventory_file.path())
        .arg(playbook.strip_prefix(&ansible_dir).unwrap_or(playbook))
        .arg("--limit")
        .arg(&host.name)
        .arg("--extra-vars")
        .arg(format!("@{}", vars_file.path().display()))
        .arg("-e")
        .arg(format!("ansible_user={}", host.user))
        .arg("-e")
        .arg(format!("ansible_port={}", host.port))
        .arg("--ask-pass")
        .status()
        .wrap_err("Failed to execute ansible-playbook")?;

    Ok(AnsibleResult {
        success: status.success(),
        exit_code: status.code().unwrap_or(-1),
        last_output: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_write_inventory_file_generates_valid_yaml() {
        let host = InventoryHost {
            name: "testhost".to_string(),
            address: "198.51.100.1".to_string(),
            port: 59865,
            user: "root".to_string(),
            groups: vec![],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let host_entry = &parsed["all"]["children"]["vps"]["hosts"]["testhost"];
        assert_eq!(host_entry["ansible_host"].as_str().unwrap(), "198.51.100.1");
        assert_eq!(host_entry["ansible_port"].as_u64().unwrap(), 59865);
    }

    #[test]
    fn test_write_inventory_file_places_host_in_vps_group() {
        let host = InventoryHost {
            name: "myserver".to_string(),
            address: "203.0.113.42".to_string(),
            port: 22,
            user: "debian".to_string(),
            groups: vec![],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        assert!(parsed["all"]["children"]["vps"]["hosts"]["myserver"].is_mapping());
    }

    #[test]
    fn test_write_inventory_file_emits_tag_groups() {
        let host = InventoryHost {
            name: "openclaw".to_string(),
            address: "203.0.113.7".to_string(),
            port: 22,
            user: "root".to_string(),
            groups: vec!["hermes".to_string(), "gpu".to_string()],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let children = parsed["all"]["children"].as_mapping().unwrap();
        assert!(children.contains_key("hermes"));
        assert!(children.contains_key("gpu"));
        assert!(
            parsed["all"]["children"]["hermes"]["hosts"]
                .as_mapping()
                .unwrap()
                .contains_key("openclaw")
        );
        assert!(
            parsed["all"]["children"]["gpu"]["hosts"]
                .as_mapping()
                .unwrap()
                .contains_key("openclaw")
        );
    }

    #[test]
    fn test_write_inventory_file_no_tags_yields_only_vps_group() {
        let host = InventoryHost {
            name: "plain".to_string(),
            address: "203.0.113.8".to_string(),
            port: 22,
            user: "root".to_string(),
            groups: vec![],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let children = parsed["all"]["children"].as_mapping().unwrap();
        assert_eq!(children.len(), 1);
        assert!(children.contains_key("vps"));
    }

    #[test]
    fn test_write_inventory_file_vps_tag_does_not_clobber_vps_group() {
        let host = InventoryHost {
            name: "tagged-vps".to_string(),
            address: "203.0.113.9".to_string(),
            port: 2222,
            user: "root".to_string(),
            groups: vec!["vps".to_string(), "vps".to_string()],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let children = parsed["all"]["children"].as_mapping().unwrap();
        assert_eq!(children.len(), 1);
        let host_entry = &parsed["all"]["children"]["vps"]["hosts"]["tagged-vps"];
        assert_eq!(host_entry["ansible_host"].as_str().unwrap(), "203.0.113.9");
        assert_eq!(host_entry["ansible_port"].as_u64().unwrap(), 2222);
    }

    fn digest_of(lines: &[&str]) -> FailureDigest {
        let mut digest = FailureDigest::default();
        for line in lines {
            digest.observe(line);
        }
        digest
    }

    #[test]
    fn failure_digest_keeps_fatal_line_after_long_output() {
        let ok_lines: Vec<String> = (0..100)
            .map(|i| format!("ok: [auberge] task {i}"))
            .collect();
        let mut lines: Vec<&str> = ok_lines.iter().map(String::as_str).collect();
        lines.push(r#"fatal: [auberge]: FAILED! => {"msg": "lego run failed", "rc": 1}"#);

        let digest = digest_of(&lines);
        assert!(digest.render().contains("lego run failed"));
    }

    #[test]
    fn failure_digest_captures_failed_item_lines() {
        let digest = digest_of(&[
            r#"failed: [auberge] (item=example.com) => {"msg": "boom"}"#,
            r#"fatal: [auberge]: FAILED! => {"msg": "All items completed"}"#,
        ]);
        let rendered = digest.render();
        assert!(rendered.contains("item=example.com"));
        assert!(rendered.contains("All items completed"));
    }

    #[test]
    fn failure_digest_captures_unreachable_line() {
        let digest = digest_of(&[
            r#"fatal: [auberge]: UNREACHABLE! => {"msg": "Failed to connect via ssh"}"#,
        ]);
        assert!(digest.render().contains("UNREACHABLE"));
    }

    #[test]
    fn failure_digest_has_no_failures_without_failure_lines() {
        let digest = digest_of(&[
            "PLAY [vps] ****",
            "TASK [Gathering Facts] ****",
            "ok: [auberge]",
            "changed: [auberge]",
        ]);
        assert!(!digest.has_failures());
    }

    #[test]
    fn failure_digest_drops_fatal_cancelled_by_ignoring() {
        let digest = digest_of(&[
            r#"fatal: [auberge]: FAILED! => {"msg": "expected failure"}"#,
            "...ignoring",
        ]);
        assert!(!digest.has_failures());
    }

    #[test]
    fn failure_digest_ignoring_cancels_all_items_of_ignored_loop_task() {
        let digest = digest_of(&[
            r#"failed: [auberge] (item=a) => {"msg": "boom a"}"#,
            r#"failed: [auberge] (item=b) => {"msg": "boom b"}"#,
            r#"failed: [auberge] (item=c) => {"msg": "boom c"}"#,
            "...ignoring",
        ]);
        assert!(!digest.has_failures());
    }

    #[test]
    fn failure_digest_ignoring_does_not_cancel_failures_of_earlier_tasks() {
        let digest = digest_of(&[
            "TASK [lego : run] ****",
            r#"fatal: [auberge]: FAILED! => {"msg": "real failure"}"#,
            "TASK [lego : cleanup] ****",
            r#"failed: [auberge] (item=x) => {"msg": "ignored"}"#,
            "...ignoring",
        ]);
        let rendered = digest.render();
        assert!(rendered.contains("real failure"));
        assert!(!rendered.contains("ignored"));
    }

    #[test]
    fn failure_digest_ignoring_only_cancels_immediately_preceding_run() {
        let digest = digest_of(&[
            r#"fatal: [auberge]: FAILED! => {"msg": "real failure"}"#,
            "ok: [auberge]",
            "...ignoring",
        ]);
        assert!(digest.render().contains("real failure"));
    }

    #[test]
    fn failure_digest_captures_play_recap() {
        let digest = digest_of(&[
            r#"fatal: [auberge]: FAILED! => {"msg": "boom"}"#,
            "PLAY RECAP *********************",
            "auberge : ok=12 changed=3 unreachable=0 failed=1 skipped=2",
        ]);
        let rendered = digest.render();
        assert!(rendered.contains("PLAY RECAP"));
        assert!(rendered.contains("failed=1"));
    }

    #[test]
    fn failure_digest_recap_alone_has_no_failures() {
        let digest = digest_of(&[
            "PLAY RECAP *********************",
            "auberge : ok=12 changed=3 unreachable=1 failed=0",
        ]);
        assert!(!digest.has_failures());
    }

    #[test]
    fn failure_digest_renders_failures_before_recap() {
        let digest = digest_of(&["PLAY RECAP ****", "auberge : ok=1 failed=1"]);
        let mut with_failure = digest_of(&[r#"fatal: [auberge]: FAILED! => {"msg": "boom"}"#]);
        with_failure.observe("PLAY RECAP ****");
        with_failure.observe("auberge : ok=1 failed=1");

        let rendered = with_failure.render();
        let fatal_pos = rendered.find("fatal:").unwrap();
        let recap_pos = rendered.find("PLAY RECAP").unwrap();
        assert!(fatal_pos < recap_pos);
        assert!(digest.render().starts_with("PLAY RECAP"));
    }

    #[test]
    fn failure_digest_caps_failure_lines() {
        let failure_lines: Vec<String> = (0..MAX_FAILURE_LINES + 5)
            .map(|i| format!(r#"fatal: [auberge]: FAILED! => {{"msg": "failure {i}"}}"#))
            .collect();
        let lines: Vec<&str> = failure_lines.iter().map(String::as_str).collect();

        let digest = digest_of(&lines);
        let rendered = digest.render();
        assert_eq!(rendered.lines().count(), MAX_FAILURE_LINES);
        assert!(rendered.contains("failure 0"));
        assert!(!rendered.contains(&format!("failure {MAX_FAILURE_LINES}")));
    }

    #[test]
    fn failure_digest_ignoring_past_cap_does_not_pop_kept_failure() {
        let mut digest = FailureDigest::default();
        for i in 0..MAX_FAILURE_LINES {
            digest.observe(&format!("TASK [role : task {i}] ****"));
            digest.observe(&format!(
                r#"fatal: [auberge]: FAILED! => {{"msg": "failure {i}"}}"#
            ));
        }
        digest.observe("TASK [role : over cap] ****");
        digest.observe(r#"fatal: [auberge]: FAILED! => {"msg": "over cap"}"#);
        digest.observe("...ignoring");

        let rendered = digest.render();
        assert_eq!(rendered.lines().count(), MAX_FAILURE_LINES);
        assert!(rendered.contains(&format!("failure {}", MAX_FAILURE_LINES - 1)));
    }

    #[test]
    fn pin_output_format_forces_default_callback_and_json_results() {
        let mut cmd = Command::new("ansible-playbook");
        pin_output_format(&mut cmd);

        let envs: Vec<(String, String)> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_str()?.to_string(),
                    v.and_then(|v| v.to_str())?.to_string(),
                ))
            })
            .collect();
        assert!(envs.contains(&(
            "ANSIBLE_STDOUT_CALLBACK".to_string(),
            "ansible.builtin.default".to_string()
        )));
        assert!(envs.contains(&(
            "ANSIBLE_CALLBACK_RESULT_FORMAT".to_string(),
            "json".to_string()
        )));
    }

    #[test]
    fn parse_ansible_task_extracts_name() {
        assert_eq!(
            parse_ansible_task("TASK [Install nginx] ***************************"),
            Some("Install nginx".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_with_role_prefix() {
        assert_eq!(
            parse_ansible_task("TASK [role : subtask name] ****"),
            Some("role : subtask name".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_gathering_facts() {
        assert_eq!(
            parse_ansible_task("TASK [Gathering Facts] *****"),
            Some("Gathering Facts".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_strips_leading_whitespace() {
        assert_eq!(
            parse_ansible_task("  TASK [Install nginx] ****"),
            Some("Install nginx".to_string())
        );
    }

    #[test]
    fn parse_ansible_task_play_line_returns_none() {
        assert!(parse_ansible_task("PLAY [all] ****").is_none());
    }

    #[test]
    fn parse_ansible_task_ok_line_returns_none() {
        assert!(parse_ansible_task("ok: [hostname]").is_none());
    }

    #[test]
    fn parse_ansible_task_empty_returns_none() {
        assert!(parse_ansible_task("").is_none());
    }

    #[test]
    fn format_ansible_task_dims_role_prefix() {
        let formatted = format_ansible_task("nginx : Install package");
        assert!(formatted.contains("nginx:"));
        assert!(formatted.contains("Install package"));
    }

    #[test]
    fn format_ansible_task_no_role_returns_unchanged() {
        let formatted = format_ansible_task("Gathering Facts");
        assert_eq!(formatted, "Gathering Facts");
    }

    #[test]
    fn format_ansible_task_nested_role_splits_on_first_separator() {
        let formatted = format_ansible_task("role : sub : detail");
        assert!(formatted.contains("role:"));
        assert!(formatted.contains("sub : detail"));
    }

    #[test]
    fn extra_var_args_formats_each_pair_as_key_equals_value() {
        let vars = [("actual_version", "26.8.0"), ("ansible_user", "debian")];
        assert_eq!(
            extra_var_args(Some(&vars)),
            vec!["actual_version=26.8.0", "ansible_user=debian"]
        );
    }

    #[test]
    fn extra_var_args_none_yields_no_args() {
        assert!(extra_var_args(None).is_empty());
    }

    #[test]
    fn test_write_inventory_file_escapes_special_chars() {
        let host = InventoryHost {
            name: "host:with#special".to_string(),
            address: "198.51.100.1".to_string(),
            port: 22,
            user: "root".to_string(),
            groups: vec![],
        };

        let tmpfile = write_inventory_file(&host).unwrap();
        let contents = std::fs::read_to_string(tmpfile.path()).unwrap();

        let parsed: serde_yaml::Value = serde_yaml::from_str(&contents).unwrap();
        let host_entry = &parsed["all"]["children"]["vps"]["hosts"]["host:with#special"];
        assert_eq!(host_entry["ansible_host"].as_str().unwrap(), "198.51.100.1");
    }
}
