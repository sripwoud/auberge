use chrono::{DateTime, TimeDelta, Utc};
use eyre::{Result, eyre};
use serde::Deserialize;
use std::fmt;

pub const CHECK_REACHABLE: &str = "repository_reachable";
pub const CHECK_SNAPSHOT: &str = "snapshot_exists";
pub const CHECK_CONTAINS_APP: &str = "contains_app";
pub const CHECK_FRESH: &str = "fresh";

/// Freshness threshold for the latest snapshot, e.g. `24h`.
///
/// Keeps the operator's literal spelling so the checklist echoes what they
/// asked for (`younger than 90m`, not a re-rendered `1h`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaxAge {
    duration: TimeDelta,
    label: String,
}

impl MaxAge {
    pub fn parse(input: &str) -> Result<Self> {
        let label = input.trim();
        let invalid =
            || eyre!("Invalid --max-age '{input}': expected <number><s|m|h|d>, for example 24h");

        let mut chars = label.chars();
        let unit = chars.next_back().ok_or_else(invalid)?;
        let value: i64 = chars.as_str().parse().map_err(|_| invalid())?;
        if value < 0 {
            return Err(invalid());
        }

        let duration = match unit {
            's' => TimeDelta::try_seconds(value),
            'm' => TimeDelta::try_minutes(value),
            'h' => TimeDelta::try_hours(value),
            'd' => TimeDelta::try_days(value),
            _ => return Err(invalid()),
        }
        .ok_or_else(invalid)?;

        Ok(Self {
            duration,
            label: label.to_string(),
        })
    }

    pub fn duration(&self) -> TimeDelta {
        self.duration
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

impl fmt::Display for MaxAge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}

/// One entry of `restic snapshots --json`.
///
/// `tags` is absent from the JSON for snapshots pushed before `backup push`
/// started tagging, hence the default.
#[derive(Debug, Clone, Deserialize)]
pub struct Snapshot {
    pub id: String,
    pub time: DateTime<Utc>,
    pub paths: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

impl Snapshot {
    /// restic's `short_id`: the first 8 characters of the snapshot id.
    pub fn short_id(&self) -> &str {
        self.id.get(..8).unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Verified,
    CheckFailed,
    OperationalError,
}

impl Status {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Verified => 0,
            Self::CheckFailed => 1,
            Self::OperationalError => 2,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::CheckFailed => "check_failed",
            Self::OperationalError => "operational_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: &'static str,
    pub message: String,
    pub passed: bool,
    pub remediation: Option<String>,
}

impl Check {
    fn passed(name: &'static str, message: String) -> Self {
        Self {
            name,
            message,
            passed: true,
            remediation: None,
        }
    }

    fn failed(name: &'static str, message: String, remediation: String) -> Self {
        Self {
            name,
            message,
            passed: false,
            remediation: Some(remediation),
        }
    }
}

/// The snapshot a verdict was reached about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotSummary {
    pub id: String,
    pub short_id: String,
    pub time: DateTime<Utc>,
    pub age_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct Verdict {
    pub status: Status,
    pub checks: Vec<Check>,
    pub snapshot: Option<SnapshotSummary>,
}

impl Verdict {
    fn new(status: Status, checks: Vec<Check>, snapshot: Option<SnapshotSummary>) -> Self {
        Self {
            status,
            checks,
            snapshot,
        }
    }

    pub fn is_verified(&self) -> bool {
        self.status == Status::Verified
    }

    /// The snapshot list could not be read, so no later check ran.
    pub fn unreachable(reason: &str) -> Self {
        Self::new(
            Status::OperationalError,
            vec![Check::failed(
                CHECK_REACHABLE,
                format!("repository reachable: {}", one_line(reason)),
                "check restic is installed and restic_repository / restic_password are right (auberge config list)"
                    .to_string(),
            )],
            None,
        )
    }
}

pub struct VerifyRequest<'a> {
    pub host: &'a str,
    pub app: Option<&'a str>,
    pub max_age: &'a MaxAge,
    pub now: DateTime<Utc>,
}

/// Fail-fast checklist: snapshot list readable, a snapshot exists for the host,
/// it contains the app (only with `app`), and it is younger than `max_age`.
///
/// Without `app` the verdict is about the newest snapshot for the host. With
/// `app` it is about the newest snapshot that holds the app, which a partial
/// sync (`backup sync --apps X`) can leave behind an app-less newer push.
///
/// `contains_app` receives a candidate snapshot and the app path to look for;
/// it is the only step that needs a second restic call, once per candidate
/// until the app is found.
pub fn verdict(
    request: &VerifyRequest<'_>,
    snapshots_json: &str,
    mut contains_app: impl FnMut(&Snapshot, &str) -> Result<bool>,
) -> Verdict {
    let snapshots: Vec<Snapshot> = match serde_json::from_str(snapshots_json) {
        Ok(snapshots) => snapshots,
        Err(e) => return Verdict::unreachable(&format!("unreadable snapshot list ({e})")),
    };

    let mut checks = vec![Check::passed(
        CHECK_REACHABLE,
        "repository reachable".to_string(),
    )];

    let candidates = host_snapshots_newest_first(&snapshots, request.host);
    let Some(newest) = candidates.first() else {
        checks.push(Check::failed(
            CHECK_SNAPSHOT,
            format!("latest snapshot for {}", request.host),
            missing_snapshot_remediation(&snapshots, request.host),
        ));
        return Verdict::new(Status::CheckFailed, checks, None);
    };

    let held = match request.app {
        None => Ok(None),
        Some(app) => newest_holding_app(&candidates, app, &mut contains_app),
    };

    // Falls back to the newest push whenever the app could not be located, so a
    // failing checklist still names a snapshot and reports its age.
    let snapshot = match &held {
        Ok(Some(held)) => held.snapshot,
        _ => newest.snapshot,
    };
    let age = request.now - snapshot.time;
    let summary = Some(SnapshotSummary {
        id: snapshot.id.clone(),
        short_id: snapshot.short_id().to_string(),
        time: snapshot.time,
        age_seconds: age.num_seconds(),
    });

    // Says what the snapshot was selected for: with `--app` it may be older than
    // the newest push, and a line claiming otherwise would misread as stale.
    let selected_for = match (request.app, &held) {
        (Some(app), Ok(Some(_))) => format!("latest snapshot containing {app}"),
        _ => format!("latest snapshot for {}", request.host),
    };
    checks.push(Check::passed(
        CHECK_SNAPSHOT,
        format!(
            "{selected_for}: {} ({}, {} ago)",
            snapshot.short_id(),
            snapshot.time.format("%Y-%m-%dT%H:%MZ"),
            format_age(age),
        ),
    ));

    if let Some(app) = request.app {
        match held {
            Err(e) => {
                checks.push(Check::failed(
                    CHECK_CONTAINS_APP,
                    format!("contains {app}: {}", one_line(&format!("{e:#}"))),
                    "check the restic repository is readable".to_string(),
                ));
                return Verdict::new(Status::OperationalError, checks, summary);
            }
            Ok(None) => {
                checks.push(Check::failed(
                    CHECK_CONTAINS_APP,
                    format!("contains {app}"),
                    format!(
                        "run: auberge backup sync --host {} --apps {app}",
                        request.host
                    ),
                ));
                return Verdict::new(Status::CheckFailed, checks, summary);
            }
            Ok(Some(held)) => checks.push(Check::passed(
                CHECK_CONTAINS_APP,
                format!("contains {app} ({})", abbreviate(&held.app_path)),
            )),
        }
    }

    let message = format!("younger than {}", request.max_age);
    if age > request.max_age.duration() {
        checks.push(Check::failed(
            CHECK_FRESH,
            message,
            format!("run: auberge backup sync --host {}", request.host),
        ));
        return Verdict::new(Status::CheckFailed, checks, summary);
    }
    checks.push(Check::passed(CHECK_FRESH, message));

    Verdict::new(Status::Verified, checks, summary)
}

struct HostSnapshot<'a> {
    snapshot: &'a Snapshot,
    root: &'a str,
}

/// The snapshot an app was found in, and the path that proved it.
struct HeldApp<'a> {
    snapshot: &'a Snapshot,
    app_path: String,
}

/// Snapshots belonging to `host`, newest first.
fn host_snapshots_newest_first<'a>(snapshots: &'a [Snapshot], host: &str) -> Vec<HostSnapshot<'a>> {
    let mut candidates: Vec<HostSnapshot<'a>> = snapshots
        .iter()
        .filter_map(|snapshot| host_snapshot(snapshot, host))
        .collect();
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.snapshot.time));
    candidates
}

/// Newest candidate holding `app`.
///
/// The walk stops at the first hit, so a repository synced in full costs one
/// probe. A probe error aborts it instead of falling through to an older
/// snapshot: a repository that cannot be read must not read as "app missing".
fn newest_holding_app<'a>(
    candidates: &[HostSnapshot<'a>],
    app: &str,
    contains_app: &mut impl FnMut(&Snapshot, &str) -> Result<bool>,
) -> Result<Option<HeldApp<'a>>> {
    for candidate in candidates {
        let app_path = format!("{}/{}", candidate.root.trim_end_matches('/'), app);
        if contains_app(candidate.snapshot, &app_path)? {
            return Ok(Some(HeldApp {
                snapshot: candidate.snapshot,
                app_path,
            }));
        }
    }
    Ok(None)
}

/// A snapshot belongs to a Host if `backup push` tagged it with the Host name,
/// or — for snapshots pushed before tagging landed — if its path carries the
/// Host segment. The same tag is what `backup prune` groups retention by.
fn host_snapshot<'a>(snapshot: &'a Snapshot, host: &str) -> Option<HostSnapshot<'a>> {
    let by_path = snapshot
        .paths
        .iter()
        .find(|path| host_from_path(path) == Some(host));

    if by_path.is_none() && !snapshot.tags.iter().any(|tag| tag == host) {
        return None;
    }

    let root = by_path.or_else(|| snapshot.paths.first())?;
    Some(HostSnapshot {
        snapshot,
        root: root.as_str(),
    })
}

/// `…/backups/<host>/<timestamp>` → `<host>`.
fn host_from_path(path: &str) -> Option<&str> {
    let mut segments = path.trim_end_matches('/').rsplit('/');
    let _timestamp = segments.next()?;
    let host = segments.next()?;
    (segments.next()? == "backups").then_some(host)
}

/// Names the hosts the repository does hold snapshots for, by the same rule
/// membership uses, so the hint never contradicts the check above it.
fn missing_snapshot_remediation(snapshots: &[Snapshot], host: &str) -> String {
    let mut hosts: Vec<&str> = snapshots
        .iter()
        .flat_map(|snapshot| {
            snapshot
                .paths
                .iter()
                .filter_map(|path| host_from_path(path))
                .chain(snapshot.tags.iter().map(String::as_str))
        })
        .collect();
    hosts.sort_unstable();
    hosts.dedup();

    match hosts.is_empty() {
        true => {
            format!("repository holds no auberge backups — run: auberge backup sync --host {host}")
        }
        false => format!(
            "repository holds snapshots for {} — run: auberge backup sync --host {host}",
            hosts.join(", ")
        ),
    }
}

fn format_age(age: TimeDelta) -> String {
    if age < TimeDelta::zero() {
        return format!("-{}", format_age(-age));
    }

    let seconds = age.num_seconds();
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => match ((s % 86_400) / 3600, s / 86_400) {
            (0, days) => format!("{days}d"),
            (hours, days) => format!("{days}d {hours}h"),
        },
    }
}

/// Keeps the last three segments: `…/myserver/2026-07-29_03-00-00/bichon`.
fn abbreviate(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    match segments.len() > 3 {
        true => format!("…/{}", segments[segments.len() - 3..].join("/")),
        false => path.to_string(),
    }
}

/// Collapses restic's multi-line diagnostics into one checklist line.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const ROOT: &str = "/home/op/.local/share/auberge/backups";

    fn now() -> DateTime<Utc> {
        "2026-07-29T09:00:00Z".parse().unwrap()
    }

    fn snapshots_json(entries: &[(&str, &str, &str)]) -> String {
        let entries: Vec<String> = entries
            .iter()
            .map(|(id, time, path)| {
                format!(r#"{{"id":"{id}","time":"{time}","paths":["{path}"]}}"#)
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    fn tagged_snapshots_json(entries: &[(&str, &str, &str, &str)]) -> String {
        let entries: Vec<String> = entries
            .iter()
            .map(|(id, time, path, tag)| {
                format!(r#"{{"id":"{id}","time":"{time}","paths":["{path}"],"tags":["{tag}"]}}"#)
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    fn myserver_snapshots() -> String {
        snapshots_json(&[(
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0a1b2",
            "2026-07-29T03:00:00Z",
            &format!("{ROOT}/myserver/2026-07-29_03-00-00"),
        )])
    }

    /// A full sync at 00:00 holding every app, then a 06:00 partial sync holding
    /// only `paperless` — the shape that used to false-alarm every other app.
    fn partial_sync_snapshots() -> String {
        snapshots_json(&[
            (
                "part2222bbbb",
                "2026-07-29T06:00:00Z",
                &format!("{ROOT}/myserver/2026-07-29_06-00-00"),
            ),
            (
                "full1111aaaa",
                "2026-07-29T00:00:00Z",
                &format!("{ROOT}/myserver/2026-07-29_00-00-00"),
            ),
        ])
    }

    /// Answers the containment probe from the full sync only, and records the
    /// order candidates were probed in.
    fn bichon_in_full_sync(
        probed: &RefCell<Vec<String>>,
    ) -> impl FnMut(&Snapshot, &str) -> Result<bool> + '_ {
        |snapshot, path| {
            probed.borrow_mut().push(snapshot.short_id().to_string());
            Ok(path == format!("{ROOT}/myserver/2026-07-29_00-00-00/bichon"))
        }
    }

    fn newest_for_host<'a>(snapshots: &'a [Snapshot], host: &str) -> Option<HostSnapshot<'a>> {
        host_snapshots_newest_first(snapshots, host)
            .into_iter()
            .next()
    }

    fn request<'a>(host: &'a str, app: Option<&'a str>, max_age: &'a MaxAge) -> VerifyRequest<'a> {
        VerifyRequest {
            host,
            app,
            max_age,
            now: now(),
        }
    }

    fn max_age(input: &str) -> MaxAge {
        MaxAge::parse(input).unwrap()
    }

    fn check<'a>(verdict: &'a Verdict, name: &str) -> Option<&'a Check> {
        verdict.checks.iter().find(|c| c.name == name)
    }

    #[test]
    fn max_age_parses_every_unit() {
        assert_eq!(max_age("45s").duration(), TimeDelta::seconds(45));
        assert_eq!(max_age("90m").duration(), TimeDelta::minutes(90));
        assert_eq!(max_age("24h").duration(), TimeDelta::hours(24));
        assert_eq!(max_age("2d").duration(), TimeDelta::days(2));
    }

    #[test]
    fn max_age_keeps_the_operators_spelling() {
        assert_eq!(max_age("90m").label(), "90m");
        assert_eq!(max_age(" 24h ").to_string(), "24h");
    }

    #[test]
    fn max_age_zero_is_allowed() {
        assert_eq!(max_age("0h").duration(), TimeDelta::zero());
    }

    #[test]
    fn max_age_rejects_malformed_input() {
        for input in ["", "24", "h", "abc", "-1h", "24x", "1.5h", "24 h"] {
            assert!(
                MaxAge::parse(input).is_err(),
                "expected '{input}' to be rejected"
            );
        }
    }

    #[test]
    fn max_age_rejects_multibyte_unit_without_panicking() {
        assert!(MaxAge::parse("24é").is_err());
    }

    #[test]
    fn max_age_rejects_overflowing_value() {
        assert!(MaxAge::parse("9999999999999999999d").is_err());
    }

    #[test]
    fn host_from_path_reads_the_host_segment() {
        assert_eq!(
            host_from_path(&format!("{ROOT}/myserver/2026-07-29_03-00-00")),
            Some("myserver")
        );
    }

    #[test]
    fn host_from_path_ignores_a_trailing_slash() {
        assert_eq!(
            host_from_path(&format!("{ROOT}/myserver/2026-07-29_03-00-00/")),
            Some("myserver")
        );
    }

    #[test]
    fn host_from_path_requires_a_backups_grandparent() {
        assert_eq!(host_from_path("/srv/mybackups/myserver/ts"), None);
        assert_eq!(host_from_path("/srv/other/myserver/ts"), None);
        assert_eq!(host_from_path("/backups/myserver"), None);
    }

    #[test]
    fn host_snapshots_are_ordered_newest_first_regardless_of_array_order() {
        let json = snapshots_json(&[
            (
                "aaa",
                "2026-07-28T03:00:00Z",
                &format!("{ROOT}/myserver/2026-07-28_03-00-00"),
            ),
            (
                "ccc",
                "2026-07-29T03:00:00Z",
                &format!("{ROOT}/myserver/2026-07-29_03-00-00"),
            ),
            (
                "bbb",
                "2026-07-27T03:00:00Z",
                &format!("{ROOT}/myserver/2026-07-27_03-00-00"),
            ),
        ]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();

        let candidates = host_snapshots_newest_first(&snapshots, "myserver");

        assert_eq!(
            candidates
                .iter()
                .map(|c| c.snapshot.id.as_str())
                .collect::<Vec<_>>(),
            vec!["ccc", "aaa", "bbb"]
        );
        assert_eq!(
            candidates[0].root,
            format!("{ROOT}/myserver/2026-07-29_03-00-00")
        );
    }

    #[test]
    fn host_snapshots_ignore_other_hosts() {
        let json = snapshots_json(&[
            (
                "aaa",
                "2026-07-29T03:00:00Z",
                &format!("{ROOT}/other/2026-07-29_03-00-00"),
            ),
            (
                "bbb",
                "2026-07-28T03:00:00Z",
                &format!("{ROOT}/myserver/2026-07-28_03-00-00"),
            ),
        ]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            newest_for_host(&snapshots, "myserver").unwrap().snapshot.id,
            "bbb"
        );
        assert!(host_snapshots_newest_first(&snapshots, "absent").is_empty());
    }

    #[test]
    fn host_snapshot_matches_the_push_tag() {
        let json = tagged_snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00Z",
            "/srv/staging/2026-07-29_03-00-00",
            "myserver",
        )]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();

        let latest = newest_for_host(&snapshots, "myserver").unwrap();

        assert_eq!(latest.snapshot.id, "aaa");
        assert_eq!(latest.root, "/srv/staging/2026-07-29_03-00-00");
    }

    #[test]
    fn host_snapshot_ignores_a_tag_naming_another_host() {
        let json = tagged_snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00Z",
            "/srv/staging/2026-07-29_03-00-00",
            "other",
        )]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();

        assert!(newest_for_host(&snapshots, "myserver").is_none());
    }

    #[test]
    fn host_snapshot_prefers_the_host_path_as_root_when_both_agree() {
        let json = tagged_snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00Z",
            &format!("{ROOT}/myserver/2026-07-29_03-00-00"),
            "myserver",
        )]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();

        assert_eq!(
            newest_for_host(&snapshots, "myserver").unwrap().root,
            format!("{ROOT}/myserver/2026-07-29_03-00-00")
        );
    }

    #[test]
    fn host_snapshot_reads_untagged_snapshots_pushed_before_tagging() {
        let snapshots: Vec<Snapshot> = serde_json::from_str(&myserver_snapshots()).unwrap();

        assert!(snapshots[0].tags.is_empty());
        assert!(newest_for_host(&snapshots, "myserver").is_some());
    }

    #[test]
    fn short_id_is_the_first_eight_characters() {
        let snapshots: Vec<Snapshot> = serde_json::from_str(&myserver_snapshots()).unwrap();
        assert_eq!(snapshots[0].short_id(), "a1b2c3d4");
    }

    #[test]
    fn snapshot_time_keeps_sub_second_precision() {
        let json = snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00.123456789Z",
            &format!("{ROOT}/myserver/2026-07-29_03-00-00"),
        )]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            (now() - snapshots[0].time).num_seconds(),
            6 * 3600 - 1,
            "age truncates toward zero"
        );
    }

    #[test]
    fn snapshot_time_with_offset_is_normalised_to_utc() {
        let json = snapshots_json(&[(
            "aaa",
            "2026-07-29T05:00:00+02:00",
            &format!("{ROOT}/myserver/2026-07-29_03-00-00"),
        )]);
        let snapshots: Vec<Snapshot> = serde_json::from_str(&json).unwrap();
        assert_eq!(
            snapshots[0].time,
            "2026-07-29T03:00:00Z".parse::<DateTime<Utc>>().unwrap()
        );
    }

    #[test]
    fn verdict_passes_when_snapshot_is_fresh() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", None, &age),
            &myserver_snapshots(),
            |_, _| panic!("containment must not be probed without an app"),
        );

        assert_eq!(verdict.status, Status::Verified);
        assert_eq!(verdict.status.exit_code(), 0);
        assert!(verdict.is_verified());
        assert!(verdict.checks.iter().all(|c| c.passed));
        assert_eq!(
            verdict.checks.iter().map(|c| c.name).collect::<Vec<_>>(),
            vec![CHECK_REACHABLE, CHECK_SNAPSHOT, CHECK_FRESH]
        );
    }

    #[test]
    fn verdict_reports_snapshot_id_time_and_age() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", None, &age),
            &myserver_snapshots(),
            |_, _| Ok(true),
        );

        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT).unwrap().message,
            "latest snapshot for myserver: a1b2c3d4 (2026-07-29T03:00Z, 6h ago)"
        );
        let summary = verdict.snapshot.unwrap();
        assert_eq!(summary.short_id, "a1b2c3d4");
        assert_eq!(summary.age_seconds, 6 * 3600);
    }

    #[test]
    fn verdict_probes_the_app_directory_of_the_resolved_snapshot() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &myserver_snapshots(),
            |snapshot, path| {
                assert_eq!(snapshot.short_id(), "a1b2c3d4");
                assert_eq!(path, format!("{ROOT}/myserver/2026-07-29_03-00-00/bichon"));
                Ok(true)
            },
        );

        assert_eq!(verdict.status, Status::Verified);
        assert_eq!(
            check(&verdict, CHECK_CONTAINS_APP).unwrap().message,
            "contains bichon (…/myserver/2026-07-29_03-00-00/bichon)"
        );
    }

    #[test]
    fn verdict_probes_only_the_newest_snapshot_when_it_holds_the_app() {
        let age = max_age("24h");
        let probed = RefCell::new(Vec::new());

        let verdict = verdict(
            &request("myserver", Some("paperless"), &age),
            &partial_sync_snapshots(),
            |snapshot, _| {
                probed.borrow_mut().push(snapshot.short_id().to_string());
                Ok(true)
            },
        );

        assert_eq!(verdict.status, Status::Verified);
        assert_eq!(*probed.borrow(), vec!["part2222"]);
    }

    #[test]
    fn verdict_verifies_the_newest_snapshot_holding_the_app_after_a_partial_sync() {
        let age = max_age("24h");
        let probed = RefCell::new(Vec::new());

        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &partial_sync_snapshots(),
            bichon_in_full_sync(&probed),
        );

        assert_eq!(verdict.status, Status::Verified);
        assert_eq!(*probed.borrow(), vec!["part2222", "full1111"]);
        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT).unwrap().message,
            "latest snapshot containing bichon: full1111 (2026-07-29T00:00Z, 9h ago)"
        );
        assert_eq!(
            check(&verdict, CHECK_CONTAINS_APP).unwrap().message,
            "contains bichon (…/myserver/2026-07-29_00-00-00/bichon)"
        );
        let summary = verdict.snapshot.unwrap();
        assert_eq!(summary.short_id, "full1111");
        assert_eq!(summary.age_seconds, 9 * 3600);
    }

    #[test]
    fn verdict_measures_freshness_against_the_snapshot_holding_the_app() {
        let age = max_age("5h");
        let probed = RefCell::new(Vec::new());

        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &partial_sync_snapshots(),
            bichon_in_full_sync(&probed),
        );

        assert_eq!(verdict.status, Status::CheckFailed);
        assert!(check(&verdict, CHECK_CONTAINS_APP).unwrap().passed);
        let failed = check(&verdict, CHECK_FRESH).unwrap();
        assert!(!failed.passed, "the 9h-old bichon snapshot is stale at 5h");
        assert_eq!(verdict.snapshot.unwrap().short_id, "full1111");
    }

    #[test]
    fn verdict_names_the_newest_push_when_no_snapshot_holds_the_app() {
        let age = max_age("24h");
        let probed = RefCell::new(Vec::new());

        let verdict = verdict(
            &request("myserver", Some("absent"), &age),
            &partial_sync_snapshots(),
            bichon_in_full_sync(&probed),
        );

        assert_eq!(verdict.status, Status::CheckFailed);
        assert_eq!(*probed.borrow(), vec!["part2222", "full1111"]);
        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT).unwrap().message,
            "latest snapshot for myserver: part2222 (2026-07-29T06:00Z, 3h ago)"
        );
        assert_eq!(
            check(&verdict, CHECK_CONTAINS_APP)
                .unwrap()
                .remediation
                .as_deref(),
            Some("run: auberge backup sync --host myserver --apps absent")
        );
    }

    #[test]
    fn verdict_ignores_older_snapshots_without_an_app() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", None, &age),
            &partial_sync_snapshots(),
            |_, _| panic!("containment must not be probed without an app"),
        );

        assert_eq!(verdict.status, Status::Verified);
        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT).unwrap().message,
            "latest snapshot for myserver: part2222 (2026-07-29T06:00Z, 3h ago)"
        );
        assert_eq!(verdict.snapshot.unwrap().short_id, "part2222");
    }

    #[test]
    fn verdict_is_operational_when_a_later_candidate_probe_fails() {
        let age = max_age("24h");

        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &partial_sync_snapshots(),
            |snapshot, _| match snapshot.short_id() {
                "part2222" => Ok(false),
                _ => Err(eyre!("restic ls failed (exit status: 1)")),
            },
        );

        assert_eq!(verdict.status, Status::OperationalError);
        assert_eq!(verdict.status.exit_code(), 2);
        assert!(check(&verdict, CHECK_FRESH).is_none());
    }

    #[test]
    fn verdict_fails_when_the_app_is_absent_from_the_snapshot() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &myserver_snapshots(),
            |_, _| Ok(false),
        );

        assert_eq!(verdict.status, Status::CheckFailed);
        assert_eq!(verdict.status.exit_code(), 1);
        let failed = check(&verdict, CHECK_CONTAINS_APP).unwrap();
        assert!(!failed.passed);
        assert_eq!(
            failed.remediation.as_deref(),
            Some("run: auberge backup sync --host myserver --apps bichon")
        );
    }

    #[test]
    fn verdict_stops_before_the_freshness_check_when_the_app_is_absent() {
        let age = max_age("1s");
        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &myserver_snapshots(),
            |_, _| Ok(false),
        );

        assert!(check(&verdict, CHECK_FRESH).is_none());
    }

    #[test]
    fn verdict_fails_when_the_snapshot_is_older_than_max_age() {
        let age = max_age("5h");
        let verdict = verdict(
            &request("myserver", None, &age),
            &myserver_snapshots(),
            |_, _| Ok(true),
        );

        assert_eq!(verdict.status, Status::CheckFailed);
        let failed = check(&verdict, CHECK_FRESH).unwrap();
        assert!(!failed.passed);
        assert_eq!(failed.message, "younger than 5h");
        assert_eq!(
            failed.remediation.as_deref(),
            Some("run: auberge backup sync --host myserver")
        );
    }

    #[test]
    fn verdict_accepts_a_snapshot_exactly_at_max_age() {
        let age = max_age("6h");
        let verdict = verdict(
            &request("myserver", None, &age),
            &myserver_snapshots(),
            |_, _| Ok(true),
        );

        assert_eq!(verdict.status, Status::Verified);
    }

    #[test]
    fn verdict_fails_when_no_snapshot_exists_for_the_host() {
        let age = max_age("24h");
        let json = snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00Z",
            &format!("{ROOT}/other/2026-07-29_03-00-00"),
        )]);

        let verdict = verdict(&request("myserver", None, &age), &json, |_, _| Ok(true));

        assert_eq!(verdict.status, Status::CheckFailed);
        assert!(verdict.snapshot.is_none());
        let failed = check(&verdict, CHECK_SNAPSHOT).unwrap();
        assert_eq!(failed.message, "latest snapshot for myserver");
        assert_eq!(
            failed.remediation.as_deref(),
            Some("repository holds snapshots for other — run: auberge backup sync --host myserver")
        );
    }

    #[test]
    fn verdict_names_tagged_hosts_in_the_remediation() {
        let age = max_age("24h");
        let json = tagged_snapshots_json(&[(
            "aaa",
            "2026-07-29T03:00:00Z",
            "/srv/staging/2026-07-29_03-00-00",
            "other",
        )]);

        let verdict = verdict(&request("myserver", None, &age), &json, |_, _| Ok(true));

        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT)
                .unwrap()
                .remediation
                .as_deref(),
            Some("repository holds snapshots for other — run: auberge backup sync --host myserver")
        );
    }

    #[test]
    fn verdict_fails_on_an_empty_repository() {
        let age = max_age("24h");
        let verdict = verdict(&request("myserver", None, &age), "[]", |_, _| Ok(true));

        assert_eq!(verdict.status, Status::CheckFailed);
        assert_eq!(
            check(&verdict, CHECK_SNAPSHOT)
                .unwrap()
                .remediation
                .as_deref(),
            Some("repository holds no auberge backups — run: auberge backup sync --host myserver")
        );
    }

    #[test]
    fn verdict_is_operational_when_the_snapshot_list_is_unreadable() {
        let age = max_age("24h");
        let verdict = verdict(&request("myserver", None, &age), "not json", |_, _| {
            Ok(true)
        });

        assert_eq!(verdict.status, Status::OperationalError);
        assert_eq!(verdict.status.exit_code(), 2);
        assert_eq!(verdict.checks.len(), 1);
        assert!(!verdict.checks[0].passed);
        assert_eq!(verdict.checks[0].name, CHECK_REACHABLE);
    }

    #[test]
    fn verdict_is_operational_when_the_containment_probe_fails() {
        let age = max_age("24h");
        let verdict = verdict(
            &request("myserver", Some("bichon"), &age),
            &myserver_snapshots(),
            |_, _| Err(eyre!("restic ls failed (exit status: 1)")),
        );

        assert_eq!(verdict.status, Status::OperationalError);
        assert_eq!(
            check(&verdict, CHECK_CONTAINS_APP).unwrap().message,
            "contains bichon: restic ls failed (exit status: 1)"
        );
        assert!(check(&verdict, CHECK_FRESH).is_none());
    }

    #[test]
    fn unreachable_verdict_collapses_multiline_restic_errors() {
        let verdict = Verdict::unreachable(
            "Fatal: repository does not exist\nIs there a repository at the following location?",
        );

        assert_eq!(verdict.status, Status::OperationalError);
        assert_eq!(
            verdict.checks[0].message,
            "repository reachable: Fatal: repository does not exist Is there a repository at the following location?"
        );
    }

    #[test]
    fn every_failed_check_carries_remediation() {
        let age = max_age("1s");
        let verdicts = [
            verdict(&request("myserver", None, &age), "[]", |_, _| Ok(true)),
            verdict(
                &request("myserver", None, &age),
                &myserver_snapshots(),
                |_, _| Ok(true),
            ),
            verdict(
                &request("myserver", Some("bichon"), &age),
                &myserver_snapshots(),
                |_, _| Ok(false),
            ),
            Verdict::unreachable("boom"),
        ];

        for verdict in verdicts {
            for failed in verdict.checks.iter().filter(|c| !c.passed) {
                assert!(
                    failed.remediation.is_some(),
                    "check '{}' failed without remediation",
                    failed.name
                );
            }
        }
    }

    #[test]
    fn format_age_scales_the_unit() {
        assert_eq!(format_age(TimeDelta::seconds(30)), "30s");
        assert_eq!(format_age(TimeDelta::minutes(5)), "5m");
        assert_eq!(format_age(TimeDelta::hours(6)), "6h");
        assert_eq!(format_age(TimeDelta::hours(47)), "1d 23h");
        assert_eq!(format_age(TimeDelta::days(2)), "2d");
    }

    #[test]
    fn format_age_marks_snapshots_dated_in_the_future() {
        assert_eq!(format_age(TimeDelta::hours(-3)), "-3h");
    }

    #[test]
    fn abbreviate_keeps_the_last_three_segments() {
        assert_eq!(
            abbreviate(&format!("{ROOT}/myserver/2026-07-29_03-00-00/bichon")),
            "…/myserver/2026-07-29_03-00-00/bichon"
        );
    }

    #[test]
    fn abbreviate_leaves_short_paths_alone() {
        assert_eq!(abbreviate("/a/b"), "/a/b");
    }

    #[test]
    fn status_strings_are_stable() {
        assert_eq!(Status::Verified.as_str(), "verified");
        assert_eq!(Status::CheckFailed.as_str(), "check_failed");
        assert_eq!(Status::OperationalError.as_str(), "operational_error");
    }
}
