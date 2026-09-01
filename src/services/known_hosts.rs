use crate::output;
use eyre::{Result, WrapErr};
use std::io::Write;
use std::path::PathBuf;
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

/// Copies whatever key `legacy_target` already holds onto `alias`, so a
/// known_hosts entry keyed by address survives the move to a name-keyed
/// `HostKeyAlias` (#785) without ssh re-verifying the target over the
/// network — `StrictHostKeyChecking accept-new` would otherwise trust
/// whatever answers under the unfamiliar alias.
///
/// A no-op — safe to call for every host on every `auberge host` mutation —
/// when `alias` already has an entry (already migrated), or when
/// `legacy_target` never did (nothing trusted yet; a real connection will
/// accept-new under the alias like any fresh host).
pub fn migrate_alias(alias: &str, legacy_target: &str) -> Result<bool> {
    if !key_lines(alias)?.is_empty() {
        return Ok(false);
    }

    let migrated: Vec<String> = key_lines(legacy_target)?
        .iter()
        .filter_map(|line| rewrite_host_field(line, alias))
        .collect();
    if migrated.is_empty() {
        return Ok(false);
    }

    append_known_hosts(&migrated)?;
    Ok(true)
}

/// The real key lines `ssh-keygen -F <target>` finds — its own `# Host ...
/// found` header stripped, so a caller never mistakes it for key material.
fn key_lines(target: &str) -> Result<Vec<String>> {
    let raw = capture(Command::new("ssh-keygen").arg("-F").arg(target))
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

fn known_hosts_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;
    Ok(home.join(".ssh/known_hosts"))
}

/// Appends `lines` to `~/.ssh/known_hosts`, guaranteeing each lands on its
/// own line even if the file's last line was left without a trailing `\n`.
fn append_known_hosts(lines: &[String]) -> Result<()> {
    let path = known_hosts_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .wrap_err_with(|| format!("Failed to create {}", parent.display()))?;
    }

    let needs_leading_newline = std::fs::read(&path)
        .map(|existing| !existing.is_empty() && !existing.ends_with(b"\n"))
        .unwrap_or(false);

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
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

/// The pre-#785 `known_hosts` key for `(address, port)`, in ssh's own
/// bracketed form for any non-default port. Only [`migrate_alias`] still
/// reads this: it is where an already-verified key lived before every
/// connection switched to a name-keyed `HostKeyAlias`.
pub fn legacy_target(address: &str, port: u16) -> String {
    if port == 22 {
        address.to_string()
    } else {
        format!("[{address}]:{port}")
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
    fn test_legacy_target_omits_port_22() {
        assert_eq!(legacy_target("198.51.100.1", 22), "198.51.100.1");
    }

    #[test]
    fn test_legacy_target_brackets_non_default_port() {
        assert_eq!(legacy_target("198.51.100.1", 2222), "[198.51.100.1]:2222");
    }

    #[test]
    fn test_legacy_target_brackets_ipv6_with_port() {
        assert_eq!(legacy_target("2001:db8::1", 2222), "[2001:db8::1]:2222");
        assert_eq!(legacy_target("2001:db8::1", 22), "2001:db8::1");
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
}
