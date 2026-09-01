use crate::output;
use eyre::{Result, WrapErr};
use std::io::Write;
use std::process::{Command, Stdio};

impl std::fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.key_type, self.hash)
    }
}

/// Compares `~/.ssh/known_hosts` against the key the target currently offers.
///
/// An unreachable, firewalled or not-yet-booted target yields `Unknown`
/// rather than an error, because ansible owns the real connection failure.
/// A broken local probe (missing `ssh-keygen`, unreadable key lines) is an
/// error: it must not be mistaken for "no known entry".
///
/// Keyed on the port bootstrap connects over. A host already hardened onto a
/// custom port keeps a separate `[ip]:port` entry, which this never inspects.
pub fn inspect(address: &str, port: u16) -> Result<HostKeyStatus> {
    let target = entry_target(address, port);

    let known_lines = capture(Command::new("ssh-keygen").arg("-F").arg(&target))
        .wrap_err("Failed to execute ssh-keygen -F")?;
    let known = fingerprints_of(&known_lines)
        .wrap_err_with(|| format!("Failed to fingerprint the known_hosts entry for {target}"))?;

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
        .wrap_err_with(|| format!("Failed to fingerprint the key offered by {target}"))?;

    if offered.is_empty() {
        return Ok(HostKeyStatus::Unknown);
    }

    Ok(classify(&known, &offered))
}

/// `ssh-keygen -R <target>` — removes every entry for the target, hashed or not.
pub fn forget(address: &str, port: u16) -> Result<()> {
    let target = entry_target(address, port);
    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-R").arg(&target);

    let result =
        output::run_piped("ssh-keygen", &mut cmd).wrap_err("Failed to execute ssh-keygen -R")?;
    if result.status.success() {
        output::clear_subprocess_lines(result.lines_written);
        Ok(())
    } else {
        Err(result.error(format!("Failed to remove known_hosts entry for {target}")))
    }
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

/// The `known_hosts` key for a target, in ssh's own bracketed form for any
/// non-default port.
pub fn entry_target(address: &str, port: u16) -> String {
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
    fn test_entry_target_omits_port_22() {
        assert_eq!(entry_target("198.51.100.1", 22), "198.51.100.1");
    }

    #[test]
    fn test_entry_target_brackets_non_default_port() {
        assert_eq!(entry_target("198.51.100.1", 2222), "[198.51.100.1]:2222");
    }

    #[test]
    fn test_entry_target_brackets_ipv6_with_port() {
        assert_eq!(entry_target("2001:db8::1", 2222), "[2001:db8::1]:2222");
        assert_eq!(entry_target("2001:db8::1", 22), "2001:db8::1");
    }
}
