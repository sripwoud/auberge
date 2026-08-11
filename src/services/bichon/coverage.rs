use crate::services::bichon::api::StoreEnvelope;
use chrono::DateTime;
use eyre::Result;
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageStatus {
    Covered,
    Gap,
}

impl CoverageStatus {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Covered => 0,
            Self::Gap => 1,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Covered => "covered",
            Self::Gap => "gap",
        }
    }
}

/// A store message the archive holds no identity evidence for, reported in
/// the canonical form the sidecar would have recorded (ADR-0013), so the
/// operator can grep the archive for it directly.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct MissingMessage {
    pub message_id: String,
    pub date: String,
    pub uid: u32,
}

/// Messages identity cannot vouch for: a body with no Message-ID header gets
/// a synthetic id in the store (regenerated on re-import) and a `sha256:` key
/// in the sidecar (hashed from the body) — the two sides can never match, so
/// they are compared by count instead. More synthetic store messages than
/// hash-keyed sidecars means at least one of them is unarchived.
#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct UnverifiableCounts {
    pub store_synthetic: usize,
    pub archive_sha256: usize,
}

#[derive(Serialize, Debug, PartialEq, Eq)]
pub struct CoverageReport {
    pub store_messages: usize,
    pub matched: usize,
    pub missing: Vec<MissingMessage>,
    pub unverifiable: UnverifiableCounts,
}

impl CoverageReport {
    pub fn status(&self) -> CoverageStatus {
        if self.missing.is_empty()
            && self.unverifiable.store_synthetic <= self.unverifiable.archive_sha256
        {
            CoverageStatus::Covered
        } else {
            CoverageStatus::Gap
        }
    }
}

/// One `.meta.json` sidecar as the Host reports it: its path (which encodes
/// the message Date as YYYY/MM), the folder observed at first sight, and the
/// canonical message identity — empty when the sidecar predates keying.
#[derive(Debug, PartialEq, Eq)]
pub struct SidecarRow {
    pub path: String,
    pub folder: String,
    pub message_id: String,
}

/// Emits "path<TAB>folder<TAB>message_id" for every sidecar of the account —
/// the same walk gate 3 of bichon-expunge.sh batches, minus its window
/// filter, which happens locally where the cutoff is already parsed. Exit 3
/// distinguishes "no such archive directory" from a broken walk.
pub fn sidecar_rows_command(archive_dir: &str) -> String {
    format!(
        r#"sudo sh -c '[ -d {archive_dir} ] || exit 3; find {archive_dir} -regextype posix-extended -regex ".*/[0-9]{{4}}/[0-9]{{2}}/[^/]+\.meta\.json" -print0 | xargs -0 -r jq -r "[input_filename, .folder, (.message_id // \"\")] | @tsv"'"#
    )
}

// The archive path reaches a single-quoted remote sh -c line; rejecting
// anything beyond safe path characters is simpler and stricter than escaping.
pub fn validate_archive_path_for_shell(path: &str) -> Result<()> {
    let ok = path.starts_with('/')
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'));
    if !ok {
        eyre::bail!("archive path '{path}' contains characters unsafe for a remote shell");
    }
    Ok(())
}

pub fn parse_sidecar_rows(stdout: &str) -> Result<Vec<SidecarRow>> {
    let mut rows = Vec::new();
    for line in stdout.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '\t');
        let (Some(path), Some(folder), Some(message_id)) =
            (parts.next(), parts.next(), parts.next())
        else {
            eyre::bail!("sidecar row the Host emitted cannot be read: {line}");
        };
        rows.push(SidecarRow {
            path: path.to_string(),
            folder: folder.to_string(),
            message_id: message_id.to_string(),
        });
    }
    Ok(rows)
}

enum StoreId {
    Synthetic,
    Canonical(String),
}

// Bichon's envelope field is the header value with angle brackets stripped,
// or — for a message with no Message-ID header — a synthetic
// `<hex.timestamp.pid@bichon>` with brackets kept (measured in ADR-0013).
// The bracket strip is defensive symmetry with the sidecar's canonical form;
// a real value has already lost them upstream.
fn canonicalize_store_id(raw: &str) -> StoreId {
    let trimmed = raw.trim();
    if trimmed.starts_with('<') && trimmed.ends_with("@bichon>") {
        return StoreId::Synthetic;
    }
    let stripped = trimmed
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(trimmed);
    StoreId::Canonical(stripped.to_string())
}

// The find regex on the Host guarantees this shape; a path that breaks it
// means the walk changed underneath this parser and must not count.
fn path_year_month(path: &str) -> Option<(i32, u32)> {
    let mut segments = path.rsplit('/');
    segments.next()?;
    let month: u32 = segments.next()?.parse().ok()?;
    let year: i32 = segments.next()?.parse().ok()?;
    Some((year, month))
}

/// Compares one folder's in-window store messages against the archive's
/// sidecar identities. `envelopes` is account-wide but already
/// window-filtered by the API query; `rows` is the account-wide sidecar walk.
/// Scoping happens here: exact folder match on both sides, and sidecars kept
/// through the cutoff month — the path encodes the message Date at month
/// granularity, the same axis the store's `date` filter bounds.
///
/// A scoped sidecar with no `message_id` predates Message-ID keying: that is
/// an unknown, not a zero, so it fails the comparison by name rather than
/// undercounting the archive into a phantom gap.
pub fn compare_coverage(
    envelopes: &[StoreEnvelope],
    rows: &[SidecarRow],
    folder: &str,
    cutoff_ym: (i32, u32),
) -> Result<CoverageReport> {
    let mut archived = HashSet::new();
    let mut archive_sha256 = 0usize;
    for row in rows {
        if row.folder != folder {
            continue;
        }
        let Some(ym) = path_year_month(&row.path) else {
            eyre::bail!(
                "archived sidecar path without a YYYY/MM partition: {}",
                row.path
            );
        };
        if ym > cutoff_ym {
            continue;
        }
        if row.message_id.is_empty() {
            eyre::bail!("an archived sidecar carries no message_id: {}", row.path);
        }
        if row.message_id.starts_with("sha256:") {
            archive_sha256 += 1;
        }
        archived.insert(row.message_id.as_str());
    }

    let mut store_messages = 0usize;
    let mut matched = 0usize;
    let mut store_synthetic = 0usize;
    let mut missing = Vec::new();
    for envelope in envelopes {
        match envelope.mailbox_name.as_deref() {
            Some(name) if name == folder => {}
            Some(_) => continue,
            None => eyre::bail!(
                "the store returned an envelope without a mailbox_name (uid {}); refusing to attribute it to a folder",
                envelope.uid
            ),
        }
        store_messages += 1;
        match canonicalize_store_id(&envelope.message_id) {
            StoreId::Synthetic => store_synthetic += 1,
            StoreId::Canonical(id) if archived.contains(id.as_str()) => matched += 1,
            StoreId::Canonical(id) => missing.push(MissingMessage {
                message_id: id,
                date: iso_date(envelope.date),
                uid: envelope.uid,
            }),
        }
    }

    Ok(CoverageReport {
        store_messages,
        matched,
        missing,
        unverifiable: UnverifiableCounts {
            store_synthetic,
            archive_sha256,
        },
    })
}

fn iso_date(epoch_ms: i64) -> String {
    DateTime::from_timestamp_millis(epoch_ms)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| format!("epoch_ms:{epoch_ms}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(message_id: &str, mailbox: Option<&str>, date: i64, uid: u32) -> StoreEnvelope {
        StoreEnvelope {
            message_id: message_id.to_string(),
            mailbox_name: mailbox.map(String::from),
            date,
            uid,
        }
    }

    fn row(path: &str, folder: &str, message_id: &str) -> SidecarRow {
        SidecarRow {
            path: path.to_string(),
            folder: folder.to_string(),
            message_id: message_id.to_string(),
        }
    }

    const CUTOFF: (i32, u32) = (2026, 5);

    #[test]
    fn every_store_message_archived_is_covered() {
        let report = compare_coverage(
            &[
                envelope("a@x.io", Some("INBOX"), 1_600_000_000_000, 1),
                envelope("b@x.io", Some("INBOX"), 1_600_000_000_000, 2),
            ],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/02/2.meta.json", "INBOX", "b@x.io"),
            ],
            "INBOX",
            CUTOFF,
        )
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Covered);
        assert_eq!(report.status().exit_code(), 0);
        assert_eq!(report.store_messages, 2);
        assert_eq!(report.matched, 2);
        assert!(report.missing.is_empty());
    }

    // The defect this closes (#400): a message the archive never ingested,
    // numerically masked by surplus sidecars of other messages, is named.
    #[test]
    fn an_unarchived_store_message_is_a_named_gap() {
        let report = compare_coverage(
            &[envelope("ghost@x.io", Some("INBOX"), 1_767_225_600_000, 42)],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX", "b@x.io"),
            ],
            "INBOX",
            CUTOFF,
        )
        .unwrap();

        assert_eq!(report.status(), CoverageStatus::Gap);
        assert_eq!(report.status().exit_code(), 1);
        assert_eq!(
            report.missing,
            vec![MissingMessage {
                message_id: "ghost@x.io".to_string(),
                date: "2026-01-01".to_string(),
                uid: 42,
            }]
        );
    }

    // Defensive symmetry: should Bichon ever keep the brackets of a real
    // header value, it still matches the sidecar's stripped canonical form.
    #[test]
    fn a_bracket_kept_store_id_matches_the_stripped_sidecar_id() {
        let report = compare_coverage(
            &[envelope("<a@x.io>", Some("INBOX"), 0, 1)],
            &[row("/a/2026/01/1.meta.json", "INBOX", "a@x.io")],
            "INBOX",
            CUTOFF,
        )
        .unwrap();
        assert_eq!(report.matched, 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn a_synthetic_store_id_is_unverifiable_not_missing() {
        let report = compare_coverage(
            &[envelope(
                "<0f2a.1778017746410.15@bichon>",
                Some("INBOX"),
                0,
                1,
            )],
            &[row("/a/2026/01/1.meta.json", "INBOX", "sha256:aa")],
            "INBOX",
            CUTOFF,
        )
        .unwrap();

        assert!(report.missing.is_empty());
        assert_eq!(report.unverifiable.store_synthetic, 1);
        assert_eq!(report.unverifiable.archive_sha256, 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn more_synthetic_store_messages_than_hash_sidecars_is_a_gap() {
        let report = compare_coverage(
            &[
                envelope("<0f2a.1.15@bichon>", Some("INBOX"), 0, 1),
                envelope("<0f2b.2.15@bichon>", Some("INBOX"), 0, 2),
            ],
            &[row("/a/2026/01/1.meta.json", "INBOX", "sha256:aa")],
            "INBOX",
            CUTOFF,
        )
        .unwrap();

        assert!(report.missing.is_empty());
        assert_eq!(report.unverifiable.store_synthetic, 2);
        assert_eq!(report.unverifiable.archive_sha256, 1);
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    // INBOX must not swallow INBOX2 on either side — the same exactness the
    // bash gate asserts of sidecar_rows_for_folder.
    #[test]
    fn folder_scoping_is_exact_on_both_sides() {
        let report = compare_coverage(
            &[
                envelope("a@x.io", Some("INBOX"), 0, 1),
                envelope("other@x.io", Some("INBOX2"), 0, 2),
            ],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "INBOX2", "other@x.io"),
            ],
            "INBOX",
            CUTOFF,
        )
        .unwrap();

        assert_eq!(report.store_messages, 1);
        assert_eq!(report.matched, 1);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    // A sidecar past the cutoff month is not evidence for the window: the
    // path month mirrors the message Date, so an in-window message can never
    // legitimately live there.
    #[test]
    fn a_sidecar_past_the_cutoff_month_is_not_evidence() {
        let report = compare_coverage(
            &[envelope("a@x.io", Some("INBOX"), 0, 1)],
            &[row("/a/2026/06/1.meta.json", "INBOX", "a@x.io")],
            "INBOX",
            CUTOFF,
        )
        .unwrap();
        assert_eq!(report.missing.len(), 1);
        assert_eq!(report.status(), CoverageStatus::Gap);
    }

    #[test]
    fn the_cutoff_month_itself_still_counts() {
        let report = compare_coverage(
            &[envelope("a@x.io", Some("INBOX"), 0, 1)],
            &[row("/a/2026/05/1.meta.json", "INBOX", "a@x.io")],
            "INBOX",
            CUTOFF,
        )
        .unwrap();
        assert_eq!(report.matched, 1);
    }

    #[test]
    fn an_unkeyed_sidecar_in_scope_fails_by_name() {
        let err = compare_coverage(
            &[],
            &[row("/a/2026/01/1.meta.json", "INBOX", "")],
            "INBOX",
            CUTOFF,
        )
        .unwrap_err();
        assert!(format!("{err}").contains("/a/2026/01/1.meta.json"));
    }

    // The bash gate only refuses unkeyed sidecars inside the probed scope;
    // this comparison must not be stricter, or a stale sidecar in an excluded
    // folder would block every expunge of the account.
    #[test]
    fn an_unkeyed_sidecar_outside_the_scope_is_ignored() {
        let report = compare_coverage(
            &[envelope("a@x.io", Some("INBOX"), 0, 1)],
            &[
                row("/a/2026/01/1.meta.json", "INBOX", "a@x.io"),
                row("/a/2026/01/2.meta.json", "Trash", ""),
                row("/a/2026/06/3.meta.json", "INBOX", ""),
            ],
            "INBOX",
            CUTOFF,
        )
        .unwrap();
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn a_store_envelope_without_a_mailbox_name_fails_loudly() {
        let err =
            compare_coverage(&[envelope("a@x.io", None, 0, 7)], &[], "INBOX", CUTOFF).unwrap_err();
        assert!(format!("{err}").contains("uid 7"));
    }

    #[test]
    fn an_empty_window_is_covered() {
        let report = compare_coverage(&[], &[], "INBOX", CUTOFF).unwrap();
        assert_eq!(report.store_messages, 0);
        assert_eq!(report.status(), CoverageStatus::Covered);
    }

    #[test]
    fn parse_sidecar_rows_keeps_an_empty_trailing_id() {
        let rows = parse_sidecar_rows(
            "/a/2026/01/1.meta.json\tINBOX\ta@x.io\n/a/2026/01/2.meta.json\tINBOX\t\n",
        )
        .unwrap();
        assert_eq!(
            rows,
            vec![
                SidecarRow {
                    path: "/a/2026/01/1.meta.json".to_string(),
                    folder: "INBOX".to_string(),
                    message_id: "a@x.io".to_string(),
                },
                SidecarRow {
                    path: "/a/2026/01/2.meta.json".to_string(),
                    folder: "INBOX".to_string(),
                    message_id: String::new(),
                },
            ]
        );
    }

    #[test]
    fn parse_sidecar_rows_refuses_a_row_missing_fields() {
        let err = parse_sidecar_rows("/a/2026/01/1.meta.json\n").unwrap_err();
        assert!(format!("{err}").contains("/a/2026/01/1.meta.json"));
    }

    #[test]
    fn parse_sidecar_rows_of_nothing_is_empty() {
        assert!(parse_sidecar_rows("").unwrap().is_empty());
    }

    #[test]
    fn sidecar_rows_command_guards_and_batches() {
        let cmd = sidecar_rows_command("/var/lib/bichon-archive/a@x.io");
        assert!(cmd.contains("[ -d /var/lib/bichon-archive/a@x.io ] || exit 3"));
        assert!(cmd.contains("xargs -0 -r jq"));
    }

    #[test]
    fn archive_path_validation_rejects_shell_metacharacters() {
        assert!(validate_archive_path_for_shell("/var/lib/bichon-archive").is_ok());
        assert!(validate_archive_path_for_shell("/var/lib/x'; rm -rf /").is_err());
        assert!(validate_archive_path_for_shell("relative/path").is_err());
        assert!(validate_archive_path_for_shell("/with space").is_err());
    }

    #[test]
    fn a_gap_from_missing_and_a_gap_from_synthetic_render_one_status() {
        let report = CoverageReport {
            store_messages: 3,
            matched: 2,
            missing: vec![MissingMessage {
                message_id: "m@x.io".to_string(),
                date: "2026-01-01".to_string(),
                uid: 1,
            }],
            unverifiable: UnverifiableCounts {
                store_synthetic: 0,
                archive_sha256: 0,
            },
        };
        assert_eq!(report.status().as_str(), "gap");
    }
}
