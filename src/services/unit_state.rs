//! The state a failed deploy left the App's units in (#644).
//!
//! A Playbook that exits non-zero says which play broke, not whether the App
//! is now serving old code, stopped, or restarting in a loop — grimmory
//! crash-looped for seven hours after a failed deploy whose probe had
//! detected exactly that. This module reads the Unit Ownership declarations
//! (`units:` in Playbook Meta) for the Apps a failed run targeted, probes
//! the Host once over ssh, and renders the verdicts an operator acts on.
//!
//! Verdicts are derived, never asserted: "still on the pre-deploy artifact"
//! and "restarted mid-deploy" come from comparing the unit's monotonic start
//! timestamp against the Host's own uptime and the locally measured deploy
//! window — one clock, no skew — and the raw systemd fields stay visible so
//! a wrong classification is auditable.

use crate::ansible_assets::AnsibleAssets;
use crate::hosts::HostManager;
use crate::playbook_meta::{OwnedUnit, UnitScope, load_all_metas};
use crate::services::dependency_resolver::{PlaybookRun, playbook_role_names};
use crate::services::ssh::{LiveSshSession, SshSession, resolve_ssh_key_path};
use eyre::{Result, WrapErr};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

/// A unit to probe, carried with the App that owns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUnit {
    pub app: String,
    pub unit: OwnedUnit,
}

/// One probed unit's raw systemd fields, as `systemctl show` answered them.
#[derive(Debug, Clone)]
struct ProbedUnit {
    app: String,
    name: String,
    load_state: String,
    active_state: String,
    sub_state: String,
    n_restarts: Option<u64>,
    /// `ExecMainStartTimestampMonotonic`, then `ActiveEnterTimestampMonotonic`
    /// for units without a main PID (timers, sockets); µs since boot, 0 = never.
    started_usec: Option<u64>,
}

/// What an operator does about each state differs; that difference is the
/// report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    ActivePreDeploy,
    RestartedMidDeploy,
    Looping,
    Failed,
    Stopped,
    NotFound,
}

/// The units the failing run owns: every App the run targeted (its tags, or
/// the whole playbook when untagged), resolved through the Unit Ownership
/// declarations. An App without a declaration contributes nothing.
pub fn units_for_run(
    run: &PlaybookRun,
    playbooks_dir: &Path,
    admin_user: &str,
) -> Result<Vec<AppUnit>> {
    let filename = run
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let mut apps: Vec<String> = if run.tags.is_empty() {
        let mut apps = playbook_role_names(filename)?;
        if let Some(stem) = filename.strip_suffix(".yml") {
            apps.push(stem.to_string());
        }
        apps
    } else {
        run.tags.clone()
    };
    apps.sort();
    apps.dedup();

    let metas: BTreeMap<String, _> = load_all_metas(playbooks_dir)?.into_iter().collect();
    let mut seen = BTreeMap::new();
    for app in apps {
        let Some(meta) = metas.get(&app) else {
            continue;
        };
        for unit in meta.owned_units(admin_user) {
            seen.entry((unit.name.clone(), unit.scope))
                .or_insert_with(|| AppUnit {
                    app: app.clone(),
                    unit,
                });
        }
    }
    Ok(seen.into_values().collect())
}

const SHOW_PROPERTIES: &str = "Id,LoadState,ActiveState,SubState,NRestarts,\
                               ExecMainStartTimestampMonotonic,ActiveEnterTimestampMonotonic";

fn show_command(units: &[&AppUnit], scope: UnitScope) -> String {
    let names: Vec<&str> = units.iter().map(|u| u.unit.name.as_str()).collect();
    match scope {
        UnitScope::System => format!(
            "systemctl show --property={SHOW_PROPERTIES} -- {}",
            names.join(" ")
        ),
        UnitScope::User => format!(
            "XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user show \
             --property={SHOW_PROPERTIES} -- {}",
            names.join(" ")
        ),
    }
}

/// `systemctl show` blocks — blank-line separated `Key=Value` groups — keyed
/// back to the probed units by `Id`.
fn parse_show_blocks(stdout: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut blocks = BTreeMap::new();
    for block in stdout.split("\n\n") {
        let fields: BTreeMap<String, String> = block
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        if let Some(id) = fields.get("Id") {
            blocks.insert(id.clone(), fields);
        }
    }
    blocks
}

fn probe_scope<S: SshSession + ?Sized>(
    session: &S,
    units: &[&AppUnit],
    scope: UnitScope,
    out: &mut Vec<ProbedUnit>,
) -> Result<()> {
    let command = show_command(units, scope);
    let result = session.run(&command)?;
    if !result.success {
        eyre::bail!(
            "`{command}` failed: {}",
            result.stderr_str().trim().to_string()
        );
    }
    let blocks = parse_show_blocks(&result.stdout_str());
    for app_unit in units {
        let fields = blocks.get(&app_unit.unit.name).cloned().unwrap_or_default();
        let get = |key: &str| fields.get(key).cloned().unwrap_or_default();
        let parsed = |key: &str| fields.get(key).and_then(|v| v.parse::<u64>().ok());
        out.push(ProbedUnit {
            app: app_unit.app.clone(),
            name: app_unit.unit.name.clone(),
            load_state: get("LoadState"),
            active_state: get("ActiveState"),
            sub_state: get("SubState"),
            n_restarts: parsed("NRestarts"),
            started_usec: parsed("ExecMainStartTimestampMonotonic")
                .filter(|&usec| usec > 0)
                .or_else(|| parsed("ActiveEnterTimestampMonotonic").filter(|&usec| usec > 0)),
        });
    }
    Ok(())
}

/// The Host's own monotonic clock, µs since boot — the clock the probed
/// timestamps are on, so no skew against the operator's machine.
fn host_uptime_usec<S: SshSession + ?Sized>(session: &S) -> Result<u64> {
    let result = session.run("cat /proc/uptime")?;
    if !result.success {
        eyre::bail!("`cat /proc/uptime` failed");
    }
    let stdout = result.stdout_str();
    let seconds: f64 = stdout
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .parse()
        .wrap_err_with(|| format!("unreadable /proc/uptime: {}", stdout.trim()))?;
    Ok((seconds * 1_000_000.0) as u64)
}

fn verdict(unit: &ProbedUnit, uptime_usec: u64, elapsed: Duration) -> Verdict {
    if unit.load_state != "loaded" {
        return Verdict::NotFound;
    }
    match unit.active_state.as_str() {
        "activating" if unit.sub_state == "auto-restart" => Verdict::Looping,
        "failed" => Verdict::Failed,
        "inactive" | "deactivating" => Verdict::Stopped,
        _ => match unit.started_usec {
            Some(started) if uptime_usec.saturating_sub(started) <= elapsed.as_micros() as u64 => {
                Verdict::RestartedMidDeploy
            }
            _ => Verdict::ActivePreDeploy,
        },
    }
}

fn age(unit: &ProbedUnit, uptime_usec: u64) -> String {
    match unit.started_usec {
        Some(started) => {
            let secs = uptime_usec.saturating_sub(started) / 1_000_000;
            if secs < 120 {
                format!("{secs}s ago")
            } else {
                format!("{}min ago", secs / 60)
            }
        }
        None => "never".to_string(),
    }
}

fn restarts(unit: &ProbedUnit) -> String {
    match unit.n_restarts {
        Some(count) => format!(", {count} restarts"),
        None => String::new(),
    }
}

/// The report: one line per unit an operator acts on, the untouched rolled
/// up. Raw `ActiveState (SubState)` stays on every detailed line so a wrong
/// verdict is auditable.
fn render(host_name: &str, probed: &[ProbedUnit], uptime_usec: u64, elapsed: Duration) -> String {
    let mut lines = vec![format!("Unit state on {host_name}:")];
    let mut untouched = Vec::new();
    for unit in probed {
        let raw = format!("{} ({})", unit.active_state, unit.sub_state);
        let line = match verdict(unit, uptime_usec, elapsed) {
            Verdict::ActivePreDeploy => {
                untouched.push(unit.name.clone());
                continue;
            }
            Verdict::Looping => format!(
                "restart-looping — {raw}{}, last start {}",
                restarts(unit),
                age(unit, uptime_usec)
            ),
            Verdict::Failed => format!("failed and gave up — {raw}{}", restarts(unit)),
            Verdict::Stopped => format!("stopped — {raw}"),
            Verdict::RestartedMidDeploy => format!(
                "restarted mid-deploy, running the new artifact — {raw}, started {}",
                age(unit, uptime_usec)
            ),
            Verdict::NotFound => format!("not found — LoadState={}", unit.load_state),
        };
        lines.push(format!("  {} ({}): {line}", unit.name, unit.app));
    }
    if !untouched.is_empty() {
        lines.push(format!(
            "  {} unit(s) untouched, active since before this deploy: {}",
            untouched.len(),
            untouched.join(", ")
        ));
    }
    lines.join("\n")
}

/// Probe every owned unit in one pass per scope and render the report.
pub fn failure_report<S: SshSession + ?Sized>(
    session: &S,
    host_name: &str,
    units: &[AppUnit],
    elapsed: Duration,
) -> Result<String> {
    let mut probed = Vec::new();
    for scope in [UnitScope::System, UnitScope::User] {
        let of_scope: Vec<&AppUnit> = units.iter().filter(|u| u.unit.scope == scope).collect();
        if !of_scope.is_empty() {
            probe_scope(session, &of_scope, scope, &mut probed)?;
        }
    }
    let uptime_usec = host_uptime_usec(session)?;
    Ok(render(host_name, &probed, uptime_usec, elapsed))
}

/// The whole readout for a failed run, shaped for the deploy error path: the
/// report, or the one honest line a failed probe earns — never an error that
/// could displace the deploy failure it annotates. `None` when the run owns
/// no units, so the happy path and unit-less playbooks stay silent.
pub fn deploy_failure_unit_report(
    run: &PlaybookRun,
    host_name: &str,
    elapsed: Duration,
) -> Option<String> {
    match try_report(run, host_name, elapsed) {
        Ok(report) => report,
        Err(err) => Some(format!("unit state probe failed: {err:#}")),
    }
}

fn try_report(run: &PlaybookRun, host_name: &str, elapsed: Duration) -> Result<Option<String>> {
    let assets = AnsibleAssets::prepare()?;
    let host = HostManager::get_host(host_name)?;
    let units = units_for_run(run, &assets.playbooks_dir(), &host.user)?;
    if units.is_empty() {
        return Ok(None);
    }
    let ssh_key = resolve_ssh_key_path(&host, None)?;
    let session = LiveSshSession::new(&host, &ssh_key);
    failure_report(&session, host_name, &units, elapsed).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ssh::{CommandResult, MockSshSession};

    fn app_unit(app: &str, name: &str, scope: UnitScope) -> AppUnit {
        AppUnit {
            app: app.to_string(),
            unit: OwnedUnit {
                name: name.to_string(),
                scope,
            },
        }
    }

    fn stdout(text: &str) -> CommandResult {
        CommandResult {
            success: true,
            exit_code: Some(0),
            stdout: text.as_bytes().to_vec(),
            stderr: Vec::new(),
        }
    }

    fn probed(
        name: &str,
        active: &str,
        sub: &str,
        n_restarts: Option<u64>,
        started_usec: Option<u64>,
    ) -> ProbedUnit {
        ProbedUnit {
            app: "radio".to_string(),
            name: name.to_string(),
            load_state: "loaded".to_string(),
            active_state: active.to_string(),
            sub_state: sub.to_string(),
            n_restarts,
            started_usec,
        }
    }

    const UPTIME: u64 = 1_000_000_000; // 1000s since boot
    const ELAPSED: Duration = Duration::from_secs(100);

    #[test]
    fn test_verdict_looping_from_auto_restart() {
        let unit = probed(
            "liquidsoap.service",
            "activating",
            "auto-restart",
            Some(4),
            None,
        );
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::Looping);
    }

    #[test]
    fn test_verdict_failed_when_the_limiter_gave_up() {
        let unit = probed("liquidsoap.service", "failed", "failed", Some(30), None);
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::Failed);
    }

    #[test]
    fn test_verdict_stopped() {
        let unit = probed("icecast2.service", "inactive", "dead", None, None);
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::Stopped);
    }

    #[test]
    fn test_verdict_active_before_the_deploy_window() {
        let unit = probed(
            "icecast2.service",
            "active",
            "running",
            Some(0),
            Some(1_000_000),
        );
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::ActivePreDeploy);
    }

    #[test]
    fn test_verdict_restarted_inside_the_deploy_window() {
        let started = UPTIME - 50 * 1_000_000; // 50s ago, deploy ran 100s
        let unit = probed(
            "grimmory.service",
            "active",
            "running",
            Some(0),
            Some(started),
        );
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::RestartedMidDeploy);
    }

    #[test]
    fn test_verdict_not_found_for_an_unloaded_unit() {
        let mut unit = probed("ghost.service", "inactive", "dead", None, None);
        unit.load_state = "not-found".to_string();
        assert_eq!(verdict(&unit, UPTIME, ELAPSED), Verdict::NotFound);
    }

    #[test]
    fn test_parse_show_blocks_keys_by_id() {
        let parsed = parse_show_blocks(
            "Id=a.service\nActiveState=active\n\nId=b.timer\nActiveState=inactive\n",
        );
        assert_eq!(parsed["a.service"]["ActiveState"], "active");
        assert_eq!(parsed["b.timer"]["ActiveState"], "inactive");
    }

    #[test]
    fn test_show_command_batches_system_units_in_one_call() {
        let liquidsoap = app_unit("radio", "liquidsoap.service", UnitScope::System);
        let icecast = app_unit("radio", "icecast2.service", UnitScope::System);
        let command = show_command(&[&liquidsoap, &icecast], UnitScope::System);
        assert!(command.starts_with("systemctl show --property="));
        assert!(command.ends_with("-- liquidsoap.service icecast2.service"));
    }

    #[test]
    fn test_show_command_probes_user_units_through_the_user_manager() {
        let hermes = app_unit("hermes", "hermes-gateway.service", UnitScope::User);
        let command = show_command(&[&hermes], UnitScope::User);
        assert!(command.contains("XDG_RUNTIME_DIR=/run/user/$(id -u)"));
        assert!(command.contains("systemctl --user show"));
        assert!(command.ends_with("-- hermes-gateway.service"));
    }

    #[test]
    fn test_failure_report_details_anomalies_and_rolls_up_the_untouched() {
        let session = MockSshSession::new();
        session.stage_run_result(stdout(
            "Id=liquidsoap.service\nLoadState=loaded\nActiveState=activating\n\
             SubState=auto-restart\nNRestarts=4\nExecMainStartTimestampMonotonic=999000000\n\n\
             Id=icecast2.service\nLoadState=loaded\nActiveState=active\nSubState=running\n\
             NRestarts=0\nExecMainStartTimestampMonotonic=1000000\n",
        ));
        session.stage_run_result(stdout("1000.00 1800.00\n"));

        let units = [
            app_unit("radio", "liquidsoap.service", UnitScope::System),
            app_unit("radio", "icecast2.service", UnitScope::System),
        ];
        let report = failure_report(&session, "auberge", &units, ELAPSED).unwrap();

        assert!(report.starts_with("Unit state on auberge:"));
        assert!(
            report.contains(
                "liquidsoap.service (radio): restart-looping — activating (auto-restart), \
                 4 restarts, last start 1s ago"
            ),
            "unexpected report: {report}"
        );
        assert!(
            report
                .contains("1 unit(s) untouched, active since before this deploy: icecast2.service"),
            "unexpected report: {report}"
        );
    }

    fn repo_playbooks_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("ansible")
            .join("playbooks")
    }

    fn run_for(playbook: &str, tags: &[&str]) -> PlaybookRun {
        PlaybookRun {
            path: repo_playbooks_dir().join(playbook),
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn test_units_for_run_reads_the_tagged_apps_declarations() {
        let run = run_for("apps.yml", &["radio"]);
        let units = units_for_run(&run, &repo_playbooks_dir(), "alice").unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.unit.name.as_str()).collect();
        assert_eq!(names, vec!["icecast2.service", "liquidsoap.service"]);
        assert!(units.iter().all(|u| u.app == "radio"));
    }

    #[test]
    fn test_units_for_run_resolves_admin_user_in_a_declaration() {
        let run = run_for("apps.yml", &["syncthing"]);
        let units = units_for_run(&run, &repo_playbooks_dir(), "alice").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].unit.name, "syncthing@alice.service");
    }

    #[test]
    fn test_units_for_run_keeps_a_user_units_scope() {
        let run = run_for("apps.yml", &["hermes"]);
        let units = units_for_run(&run, &repo_playbooks_dir(), "alice").unwrap();
        assert_eq!(units.len(), 1);
        assert_eq!(units[0].unit.name, "hermes-gateway.service");
        assert_eq!(units[0].unit.scope, UnitScope::User);
    }

    #[test]
    fn test_units_for_run_expands_an_untagged_run_to_the_whole_playbook() {
        let run = run_for("vibecoder.yml", &[]);
        let units = units_for_run(&run, &repo_playbooks_dir(), "alice").unwrap();
        let names: Vec<&str> = units.iter().map(|u| u.unit.name.as_str()).collect();
        assert!(names.contains(&"vibecoder.service"), "got: {names:?}");
    }

    #[test]
    fn test_units_for_run_is_empty_for_a_playbook_owning_no_units() {
        let run = run_for("hardening.yml", &[]);
        let units = units_for_run(&run, &repo_playbooks_dir(), "alice").unwrap();
        assert!(units.is_empty(), "got: {units:?}");
    }

    #[test]
    fn test_failure_report_propagates_a_probe_failure() {
        let session = MockSshSession::new();
        session.stage_run_result(CommandResult {
            success: false,
            exit_code: Some(255),
            stdout: Vec::new(),
            stderr: b"ssh: connect to host auberge port 22: Connection refused".to_vec(),
        });
        let units = [app_unit("radio", "liquidsoap.service", UnitScope::System)];
        let err = failure_report(&session, "auberge", &units, ELAPSED).unwrap_err();
        assert!(err.to_string().contains("Connection refused"));
    }
}
