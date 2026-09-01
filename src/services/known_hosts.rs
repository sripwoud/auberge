use crate::output;
use eyre::{Result, WrapErr};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.key_type, self.hash)
    }
}

/// Compares `~/.ssh/known_hosts` against the key the target currently offers.
///
/// `alias` is the `HostKeyAlias` every live connection now presents under
/// (#785): the known side is looked up by the Host's name, independent of
/// whichever address or port actually reaches it. `address`/`port` are only
/// where the *offered* key is scanned from.
///
/// An unreachable, firewalled or not-yet-booted target yields `Unknown`
/// rather than an error, because ansible owns the real connection failure.
/// A broken local probe (missing `ssh-keygen`, unreadable key lines) is an
/// error: it must not be mistaken for "no known entry".
pub fn inspect(alias: &str, address: &str, port: u16) -> Result<HostKeyStatus> {
    let known_lines = capture(Command::new("ssh-keygen").arg("-F").arg(alias))
        .wrap_err("Failed to execute ssh-keygen -F")?;
    let known = fingerprints_of(&known_lines)
        .wrap_err_with(|| format!("Failed to fingerprint the known_hosts entry for {alias}"))?;

    let offered_lines = capture(
        Command::new("ssh-keyscan")
            .arg("-T")
            .arg("5")
            .arg("-p")
            .arg(port.to_string())
            .arg(address),
    )
    .wrap_err("Failed to execute ssh-keyscan")?;

    let offered = fingerprints_of(&offered_lines)
        .wrap_err_with(|| format!("Failed to fingerprint the key offered by {address}:{port}"))?;

    if offered.is_empty() {
        return Ok(HostKeyStatus::Unknown);
    }

    Ok(classify(&known, &offered))
}

/// `ssh-keygen -R <alias>` — removes every entry for the alias, hashed or not.
pub fn forget(alias: &str) -> Result<()> {
    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-R").arg(alias);

    let result =
        output::run_piped("ssh-keygen", &mut cmd).wrap_err("Failed to execute ssh-keygen -R")?;
    if result.status.success() {
        output::clear_subprocess_lines(result.lines_written);
        Ok(())
    } else {
        Err(result.error(format!("Failed to remove known_hosts entry for {alias}")))
    }
}

/// Copies whatever key a `legacy_targets` entry already holds onto `alias`,
/// so a known_hosts entry keyed by address survives the move to a name-keyed
/// `HostKeyAlias` (#785) without ssh re-verifying the target over the network.
///
/// Copied rather than re-accepted, because there is nothing to re-accept it
/// with: `StrictHostKeyChecking accept-new` reaches only the generated
/// include's `Host <name>` stanza, and the CLI's transport connects to
/// `user@<address>`, so ssh's default `ask` governs and a non-interactive run
/// fails outright (#800). Opting the transport in instead would trust whatever
/// answers under an alias it has never seen (#780).
///
/// A no-op — safe to call for every host on every roster read — when `alias`
/// already has an entry (already migrated), or when none of `legacy_targets`
/// ever did (nothing trusted yet, including a `known_hosts` that does not
/// exist).
///
/// `legacy_targets` is tried in order and the first hit wins, because that is
/// how ssh resolves them: the alias must inherit the key a connection would
/// actually have been checked against.
fn migrate_alias(known_hosts: &Path, alias: &str, legacy_targets: &[String]) -> Result<bool> {
    if !key_lines(known_hosts, alias)?.is_empty() {
        return Ok(false);
    }

    for target in legacy_targets {
        let migrated: Vec<String> = key_lines(known_hosts, target)?
            .iter()
            .filter_map(|line| rewrite_host_field(line, alias))
            .collect();
        if !migrated.is_empty() {
            append_known_hosts(known_hosts, &migrated)?;
            return Ok(true);
        }
    }

    Ok(false)
}

/// Walks the whole roster through [`migrate_alias`], so every Host the user
/// has already verified keeps its trust once every connection starts
/// presenting a `HostKeyAlias` (#785).
///
/// `Host::address` here is deliberately the declaration, not a resolved
/// `Route`: it names where the key *used* to be stored, which no routing
/// policy (#787) may move.
///
/// `known_hosts` is a parameter rather than resolved here, and the indirection
/// exists for the test rather than for configurability: production passes
/// [`default_path`], and a temp file is what lets `hosts.rs` assert the
/// migration is bound to the roster read without shelling out against the
/// developer's own trust store. [`inspect`] and [`forget`] keep ssh's default
/// file — they neither write nor sit inside a binding a test has to observe.
pub fn migrate_roster(known_hosts: &Path, hosts: &[crate::hosts::Host]) -> Result<()> {
    for host in hosts {
        migrate_alias(
            known_hosts,
            &host.name,
            &legacy_targets(&host.address, host.port),
        )
        .wrap_err_with(|| {
            format!(
                "Failed to migrate the known_hosts alias for host '{}'",
                host.name
            )
        })?;
    }
    Ok(())
}

/// The real key lines `ssh-keygen -F <target>` finds in `known_hosts` — its
/// own `# Host ... found` header stripped, so a caller never mistakes it for
/// key material.
///
/// A `known_hosts` that does not exist yields no lines rather than an error:
/// `ssh-keygen -F` exits non-zero with an empty stdout, which is the same
/// answer as a miss and the correct one — nothing has been trusted yet.
fn key_lines(known_hosts: &Path, target: &str) -> Result<Vec<String>> {
    let raw = capture(
        Command::new("ssh-keygen")
            .arg("-F")
            .arg(target)
            .arg("-f")
            .arg(known_hosts),
    )
    .wrap_err_with(|| format!("Failed to run ssh-keygen -F for {target}"))?;
    Ok(strip_comment_lines(&raw))
}

fn strip_comment_lines(raw: &str) -> Vec<String> {
    raw.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// Swaps a known_hosts line's leading host field for `alias`, keeping the
/// key type, key material and any comment verbatim — hashed or not, the
/// migration never needs to decode the original field, only discard it.
fn rewrite_host_field(line: &str, alias: &str) -> Option<String> {
    let (_, rest) = line.split_once(char::is_whitespace)?;
    let rest = rest.trim_start();
    if rest.is_empty() {
        return None;
    }
    Some(format!("{alias} {rest}"))
}

/// ssh's own `known_hosts`, which is the file every real connection reads.
pub fn default_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    Ok(home.join(".ssh/known_hosts"))
}

/// Appends `lines` to `known_hosts`, guaranteeing each lands on its own line
/// even if the file's last line was left without a trailing `\n`.
fn append_known_hosts(path: &Path, lines: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create {}", parent.display()))?;
    }

    let needs_leading_newline = std::fs::read(path)
        .map(|existing| !existing.is_empty() && !existing.ends_with(b"\n"))
        .unwrap_or(false);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .wrap_err_with(|| format!("Failed to open {}", path.display()))?;

    if needs_leading_newline {
        file.write_all(b"\n")
            .wrap_err_with(|| format!("Failed to write {}", path.display()))?;
    }
    for line in lines {
        writeln!(file, "{line}").wrap_err_with(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(())
}

/// A non-zero exit is expected — `ssh-keygen -F` on a miss, `ssh-keyscan` on
/// an unreachable target — so only the spawn itself can fail here.
fn capture(cmd: &mut Command) -> Result<String> {
    let out = cmd.stderr(Stdio::null()).output()?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// `ssh-keygen -lf` reads a file, so the captured key lines go through one.
fn fingerprints_of(key_lines: &str) -> Result<Vec<Fingerprint>> {
    if key_lines.trim().is_empty() {
        return Ok(Vec::new());
    }

    let mut tmpfile = tempfile::NamedTempFile::new().wrap_err("Failed to create temp file")?;
    tmpfile
        .write_all(key_lines.as_bytes())
        .wrap_err("Failed to write host key lines")?;

    let out = Command::new("ssh-keygen")
        .arg("-lf")
        .arg(tmpfile.path())
        .stderr(Stdio::null())
        .output()
        .wrap_err("Failed to execute ssh-keygen -lf")?;
    if !out.status.success() {
        eyre::bail!("ssh-keygen -lf could not read the host key lines");
    }

    Ok(parse_fingerprints(&String::from_utf8_lossy(&out.stdout)))
}

/// Every pre-#785 `known_hosts` key `(address, port)` may be filed under,
/// most specific first. Only [`migrate_alias`] reads this: it is where an
/// already-verified key lived before every connection switched to a
/// name-keyed `HostKeyAlias`.
///
/// **Two spellings, not one.** ssh writes `[address]:port` for a non-default
/// port, but it *accepts* a bare-address entry for such a connection too —
/// OpenSSH 10.5p1 matches `135.125.107.230` for a connection on port 59865 —
/// and that is how the entries on the fleet #800 was filed against are
/// actually stored. Looking only under the bracketed form left the migration
/// running and finding nothing, on two of three hosts, with no output to say
/// so. Order matters for the same reason: where both exist ssh checks the
/// port-keyed one, so the alias must inherit that key and not a leftover from
/// a port-22 era.
pub fn legacy_targets(address: &str, port: u16) -> Vec<String> {
    if port == 22 {
        vec![address.to_string()]
    } else {
        vec![format!("[{address}]:{port}"), address.to_string()]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    Unknown,
    Unchanged,
    Changed {
        known: Vec<Fingerprint>,
        offered: Vec<Fingerprint>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fingerprint {
    pub key_type: String,
    pub hash: String,
}

/// Parses `ssh-keygen -lf` output: `<bits> <hash> <comment> (<TYPE>)`.
fn parse_fingerprints(output: &str) -> Vec<Fingerprint> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let _bits = fields.next()?;
            let hash = fields.next()?;
            if !hash.contains(':') {
                return None;
            }
            let key_type = fields
                .next_back()?
                .strip_prefix('(')?
                .strip_suffix(')')?
                .to_string();
            Some(Fingerprint {
                key_type,
                hash: hash.to_string(),
            })
        })
        .collect()
}

/// A key type present on both sides with a different hash is the only
/// contradiction: ssh refuses on that, and on nothing else.
fn classify(known: &[Fingerprint], offered: &[Fingerprint]) -> HostKeyStatus {
    if known.is_empty() {
        return HostKeyStatus::Unknown;
    }

    let contradicted = known.iter().any(|k| {
        offered
            .iter()
            .any(|o| o.key_type == k.key_type && o.hash != k.hash)
    });

    if contradicted {
        HostKeyStatus::Changed {
            known: known.to_vec(),
            offered: offered.to_vec(),
        }
    } else {
        HostKeyStatus::Unchanged
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYGEN_LF: &str = "256 SHA256:AAA no comment (ED25519)\n3072 SHA256:BBB host key (RSA)\n";

    #[test]
    fn test_parse_fingerprints_reads_type_and_hash() {
        let parsed = parse_fingerprints(KEYGEN_LF);
        assert_eq!(
            parsed,
            vec![
                Fingerprint {
                    key_type: "ED25519".to_string(),
                    hash: "SHA256:AAA".to_string(),
                },
                Fingerprint {
                    key_type: "RSA".to_string(),
                    hash: "SHA256:BBB".to_string(),
                },
            ]
        );
    }

    #[test]
    fn test_fingerprints_of_empty_input_yields_no_fingerprints() {
        assert!(fingerprints_of("").unwrap().is_empty());
    }

    #[test]
    fn test_fingerprints_of_fails_when_key_lines_are_unreadable() {
        assert!(fingerprints_of("not-a-host-key\n").is_err());
    }

    #[test]
    fn test_parse_fingerprints_ignores_garbage_lines() {
        assert!(parse_fingerprints("").is_empty());
        assert!(parse_fingerprints("# comment\n\n").is_empty());
    }

    fn fingerprint(key_type: &str, hash: &str) -> Fingerprint {
        Fingerprint {
            key_type: key_type.to_string(),
            hash: hash.to_string(),
        }
    }

    #[test]
    fn test_classify_unknown_when_no_known_entry() {
        let offered = vec![fingerprint("ED25519", "SHA256:AAA")];
        assert_eq!(classify(&[], &offered), HostKeyStatus::Unknown);
    }

    #[test]
    fn test_classify_unchanged_when_same_type_same_hash() {
        let known = vec![fingerprint("ED25519", "SHA256:AAA")];
        let offered = vec![fingerprint("ED25519", "SHA256:AAA")];
        assert_eq!(classify(&known, &offered), HostKeyStatus::Unchanged);
    }

    #[test]
    fn test_classify_unchanged_when_key_types_disjoint() {
        let known = vec![fingerprint("ED25519", "SHA256:AAA")];
        let offered = vec![fingerprint("RSA", "SHA256:BBB")];
        assert_eq!(classify(&known, &offered), HostKeyStatus::Unchanged);
    }

    #[test]
    fn test_classify_changed_on_same_type_different_hash() {
        let known = vec![fingerprint("ED25519", "SHA256:AAA")];
        let offered = vec![
            fingerprint("ED25519", "SHA256:ZZZ"),
            fingerprint("RSA", "SHA256:BBB"),
        ];
        assert_eq!(
            classify(&known, &offered),
            HostKeyStatus::Changed {
                known: known.clone(),
                offered: offered.clone(),
            }
        );
    }

    #[test]
    fn legacy_targets_on_port_22_is_the_bare_address_alone() {
        assert_eq!(legacy_targets("198.51.100.1", 22), ["198.51.100.1"]);
    }

    /// Both spellings, most specific first — ssh's own resolution order.
    #[test]
    fn legacy_targets_on_another_port_tries_the_bracketed_form_then_the_bare_one() {
        assert_eq!(
            legacy_targets("198.51.100.1", 2222),
            ["[198.51.100.1]:2222", "198.51.100.1"]
        );
    }

    #[test]
    fn legacy_targets_brackets_ipv6_with_a_port() {
        assert_eq!(
            legacy_targets("2001:db8::1", 2222),
            ["[2001:db8::1]:2222", "2001:db8::1"]
        );
        assert_eq!(legacy_targets("2001:db8::1", 22), ["2001:db8::1"]);
    }

    const KEYGEN_F_HEADER_AND_LINE: &str =
        "# Host 203.0.113.55 found: line 1 \n203.0.113.55 ssh-ed25519 AAAAKEY comment\n";

    #[test]
    fn strip_comment_lines_drops_the_ssh_keygen_f_header() {
        let lines = strip_comment_lines(KEYGEN_F_HEADER_AND_LINE);
        assert_eq!(lines, vec!["203.0.113.55 ssh-ed25519 AAAAKEY comment"]);
    }

    #[test]
    fn strip_comment_lines_drops_blank_lines() {
        assert!(strip_comment_lines("\n\n").is_empty());
    }

    #[test]
    fn strip_comment_lines_empty_input_yields_no_lines() {
        assert!(strip_comment_lines("").is_empty());
    }

    #[test]
    fn rewrite_host_field_swaps_a_plain_host_field() {
        assert_eq!(
            rewrite_host_field("203.0.113.55 ssh-ed25519 AAAAKEY", "auberge"),
            Some("auberge ssh-ed25519 AAAAKEY".to_string())
        );
    }

    #[test]
    fn rewrite_host_field_swaps_a_hashed_host_field() {
        assert_eq!(
            rewrite_host_field("|1|salt|hash| ssh-ed25519 AAAAKEY", "auberge"),
            Some("auberge ssh-ed25519 AAAAKEY".to_string())
        );
    }

    #[test]
    fn rewrite_host_field_keeps_a_trailing_comment_verbatim() {
        assert_eq!(
            rewrite_host_field("203.0.113.55 ssh-ed25519 AAAAKEY user@host", "auberge"),
            Some("auberge ssh-ed25519 AAAAKEY user@host".to_string())
        );
    }

    #[test]
    fn rewrite_host_field_none_when_the_line_has_no_key_material() {
        assert_eq!(rewrite_host_field("203.0.113.55", "auberge"), None);
    }

    #[test]
    fn rewrite_host_field_none_on_an_empty_line() {
        assert_eq!(rewrite_host_field("", "auberge"), None);
    }

    const LEGACY_KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKEYMATERIAL admin@auberge";

    /// A `known_hosts` holding one entry for `target`, at a path no other
    /// test — and no developer's `$HOME` — shares.
    fn known_hosts_holding(target: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, format!("{target} {LEGACY_KEY}\n")).unwrap();
        (dir, path)
    }

    #[test]
    fn migrate_alias_copies_a_legacy_entry_onto_the_alias() {
        let (_dir, path) = known_hosts_holding("203.0.113.10");

        assert!(migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 22)).unwrap());

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains(&format!("auberge {LEGACY_KEY}")),
            "{written}"
        );
        assert!(
            written.contains(&format!("203.0.113.10 {LEGACY_KEY}")),
            "{written}"
        );
    }

    /// The whole reason the migration is safe to run on every roster read:
    /// a second pass adds nothing.
    #[test]
    fn migrate_alias_is_a_no_op_once_the_alias_is_known() {
        let (_dir, path) = known_hosts_holding("203.0.113.10");
        migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 22)).unwrap();
        let after_first = std::fs::read_to_string(&path).unwrap();

        assert!(!migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 22)).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), after_first);
    }

    /// Nothing verified yet is not an error: a Host the operator has never
    /// connected to has no key to carry forward.
    #[test]
    fn migrate_alias_is_a_no_op_when_nothing_was_trusted() {
        let (_dir, path) = known_hosts_holding("198.51.100.7");

        assert!(!migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 22)).unwrap());
        assert!(
            !std::fs::read_to_string(&path).unwrap().contains("auberge "),
            "an unverified Host must not gain an alias entry"
        );
    }

    /// A `known_hosts` that does not exist yet is the same answer, not a
    /// crash: `ssh-keygen -F` on a missing file exits non-zero with nothing
    /// on stdout, which is exactly "nothing trusted".
    #[test]
    fn migrate_alias_is_a_no_op_when_there_is_no_known_hosts_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");

        assert!(!migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 22)).unwrap());
        assert!(
            !path.exists(),
            "the migration must not create an empty trust store"
        );
    }

    /// The pre-#785 key for a non-default port may be the bracketed form.
    #[test]
    fn migrate_alias_finds_a_legacy_entry_stored_under_the_bracketed_form() {
        let (_dir, path) = known_hosts_holding("[203.0.113.10]:2222");

        assert!(migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 2222)).unwrap());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains(&format!("auberge {LEGACY_KEY}"))
        );
    }

    /// And it may equally be the bare address, on a Host reached at a
    /// non-default port — which is how every entry on the real fleet #800 was
    /// filed against is actually stored. ssh accepts it (OpenSSH 10.5p1
    /// matched `135.125.107.230` for a connection on port 59865), so it is the
    /// trust in force and the migration has to carry it forward. Looking only
    /// under the bracketed form migrated nothing, silently, on two of three
    /// hosts.
    #[test]
    fn migrate_alias_finds_a_legacy_entry_stored_under_the_bare_address() {
        let (_dir, path) = known_hosts_holding("203.0.113.10");

        assert!(migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 2222)).unwrap());
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains(&format!("auberge {LEGACY_KEY}"))
        );
    }

    /// When both exist, the port-keyed one wins — the order ssh resolves them
    /// in, so the alias inherits the key a connection would actually have
    /// checked against rather than a leftover from a port-22 era.
    #[test]
    fn migrate_alias_prefers_the_port_keyed_entry_over_the_bare_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(
            &path,
            format!("203.0.113.10 ssh-ed25519 AAAASTALE stale\n[203.0.113.10]:2222 {LEGACY_KEY}\n"),
        )
        .unwrap();

        assert!(migrate_alias(&path, "auberge", &legacy_targets("203.0.113.10", 2222)).unwrap());

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            written.contains(&format!("auberge {LEGACY_KEY}")),
            "{written}"
        );
        assert!(
            !written.contains("auberge ssh-ed25519 AAAASTALE"),
            "{written}"
        );
    }

    #[test]
    fn migrate_roster_migrates_every_hosts_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("known_hosts");
        std::fs::write(&path, format!("203.0.113.10 {LEGACY_KEY}\n")).unwrap();

        let hosts = [
            crate::hosts::Host::fixture("auberge", None),
            crate::hosts::Host::fixture("ruche", None),
        ];
        migrate_roster(&path, &hosts).unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        for alias in ["auberge", "ruche"] {
            assert!(
                written.contains(&format!("{alias} {LEGACY_KEY}")),
                "{written}"
            );
        }
    }
}
