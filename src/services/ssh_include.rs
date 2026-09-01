use crate::hosts::Host;
use eyre::{Context, Result};
use std::path::{Path, PathBuf};

pub const INCLUDE_LINE: &str = "Include ~/.ssh/config.d/auberge.conf";

pub fn include_file_path(ssh_dir: &Path) -> PathBuf {
    ssh_dir.join("config.d/auberge.conf")
}

/// `HostKeyAlias` (#785) keys every alias's host-key check and known_hosts
/// entry on the Host's name, so a route change can never present as a
/// changed key.
pub fn render(hosts: &[Host]) -> String {
    let mut out = String::from(
        "# Managed by auberge. Regenerated from hosts.toml on every\n\
         # `auberge host add|edit|rename|remove` - do not edit by hand.\n",
    );
    for host in hosts {
        out.push_str(&format!(
            "\nHost {}\n  HostName {}\n  Port {}\n  User {}\n  IdentityFile {}\n  IdentitiesOnly yes\n  HostKeyAlias {}\n  StrictHostKeyChecking accept-new\n",
            host.name,
            host.address,
            host.port,
            host.user,
            identity_file(host),
            host.name
        ));
    }
    out
}

/// Tier 2 > tier 3 of the SSH key resolution (docs/configuration/ssh-keys.md);
/// tier 1 (`--ssh-key` flag) is per-invocation and has no place in a config
/// file. The derived path stays `~`-form: ssh expands it itself.
fn identity_file(host: &Host) -> String {
    match &host.ssh_key {
        Some(raw) => raw.clone(),
        None => format!("~/.ssh/identities/{}/{}", host.name, host.user),
    }
}

/// ssh applies the ~/.ssh/config ownership/permission check to included files
/// too, so the directory is created 0700 and the file forced to 0600.
pub fn write_include_file(ssh_dir: &Path, hosts: &[Host]) -> Result<PathBuf> {
    let path = include_file_path(ssh_dir);
    let dir = path.parent().expect("include path has a parent");
    create_private_dir(dir)?;
    std::fs::write(&path, render(hosts))
        .wrap_err_with(|| format!("Failed to write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .wrap_err_with(|| format!("Failed to set permissions on {}", path.display()))?;
    }
    Ok(path)
}

fn create_private_dir(dir: &Path) -> Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(dir)
        .wrap_err_with(|| format!("Failed to create {}", dir.display()))
}

/// A missing ~/.ssh/config legitimately means "no include yet"; any other
/// read failure is a real problem the caller must surface, not a nag trigger.
pub fn main_config_has_include(ssh_dir: &Path) -> Result<bool> {
    let path = ssh_dir.join("config");
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(has_include(&content, ssh_dir)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e).wrap_err_with(|| format!("Failed to read {}", path.display())),
    }
}

/// A glob include (`Include ~/.ssh/config.d/*.conf`) also loads the auberge
/// file, so it counts: a false "missing" would nag on every host subcommand.
pub fn has_include(main_config: &str, ssh_dir: &Path) -> bool {
    main_config.lines().any(|line| {
        let mut tokens = line.split_whitespace();
        let Some(directive) = tokens.next() else {
            return false;
        };
        if !directive.eq_ignore_ascii_case("include") {
            return false;
        }
        tokens.any(|arg| include_arg_loads_auberge_conf(arg.trim_matches('"'), ssh_dir))
    })
}

/// The directory must be ~/.ssh/config.d itself — tilde, absolute, or
/// ssh_dir-relative form (relative Include paths resolve against ~/.ssh) —
/// so an unrelated `Include /etc/ssh/config.d/*` never counts.
fn include_arg_loads_auberge_conf(arg: &str, ssh_dir: &Path) -> bool {
    let Some((dir, file)) = arg.rsplit_once('/') else {
        return false;
    };
    let dir_matches =
        dir == "~/.ssh/config.d" || dir == "config.d" || Path::new(dir) == ssh_dir.join("config.d");
    dir_matches && glob_matches(file, "auberge.conf")
}

/// The glob(3) subset ssh Include arguments use in practice: `*` and `?`.
fn glob_matches(pattern: &str, name: &str) -> bool {
    fn go(pattern: &[char], name: &[char]) -> bool {
        match pattern.split_first() {
            None => name.is_empty(),
            Some(('*', rest)) => (0..=name.len()).any(|skip| go(rest, &name[skip..])),
            Some(('?', rest)) => !name.is_empty() && go(rest, &name[1..]),
            Some((c, rest)) => name.first() == Some(c) && go(rest, &name[1..]),
        }
    }
    let pattern: Vec<char> = pattern.chars().collect();
    let name: Vec<char> = name.chars().collect();
    go(&pattern, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_host(name: &str, ssh_key: Option<&str>) -> Host {
        Host {
            name: name.to_string(),
            address: "203.0.113.10".to_string(),
            user: "ansible".to_string(),
            port: 22,
            ssh_key: ssh_key.map(str::to_string),
            tags: vec![],
            description: None,
            python_interpreter: None,
            become_method: "sudo".to_string(),
            tailscale_ip: None,
            tailnet_tag: None,
        }
    }

    #[test]
    fn render_empty_hosts_is_header_only() {
        let rendered = render(&[]);
        assert!(rendered.starts_with("# Managed by auberge"));
        assert!(!rendered.contains("\nHost "));
    }

    #[test]
    fn render_derives_identity_file_when_no_ssh_key_configured() {
        let rendered = render(&[fixture_host("auberge", None)]);
        assert!(rendered.contains("Host auberge\n"), "{rendered}");
        assert!(rendered.contains("  HostName 203.0.113.10\n"), "{rendered}");
        assert!(rendered.contains("  Port 22\n"), "{rendered}");
        assert!(rendered.contains("  User ansible\n"), "{rendered}");
        assert!(
            rendered.contains("  IdentityFile ~/.ssh/identities/auberge/ansible\n"),
            "{rendered}"
        );
        assert!(rendered.contains("  IdentitiesOnly yes\n"), "{rendered}");
        assert!(
            rendered.contains("  StrictHostKeyChecking accept-new\n"),
            "{rendered}"
        );
    }

    #[test]
    fn render_pins_the_host_key_lookup_to_the_hosts_name() {
        let rendered = render(&[fixture_host("auberge", None)]);
        assert!(rendered.contains("  HostKeyAlias auberge\n"), "{rendered}");
    }

    #[test]
    fn render_uses_configured_ssh_key_verbatim() {
        let rendered = render(&[fixture_host("vps", Some("~/.ssh/custom/key"))]);
        assert!(
            rendered.contains("  IdentityFile ~/.ssh/custom/key\n"),
            "{rendered}"
        );
    }

    #[test]
    fn render_keeps_hosts_toml_order() {
        let rendered = render(&[fixture_host("beta", None), fixture_host("alpha", None)]);
        let beta = rendered.find("Host beta").unwrap();
        let alpha = rendered.find("Host alpha").unwrap();
        assert!(beta < alpha);
    }

    const SSH_DIR: &str = "/home/x/.ssh";

    fn ssh_dir() -> &'static Path {
        Path::new(SSH_DIR)
    }

    #[test]
    fn has_include_matches_exact_line() {
        assert!(has_include(INCLUDE_LINE, ssh_dir()));
        assert!(has_include(
            "  Include ~/.ssh/config.d/auberge.conf",
            ssh_dir()
        ));
        assert!(has_include(
            "include ~/.ssh/config.d/auberge.conf",
            ssh_dir()
        ));
        assert!(has_include(
            "Include \"~/.ssh/config.d/auberge.conf\"",
            ssh_dir()
        ));
        assert!(has_include("Include config.d/auberge.conf", ssh_dir()));
        assert!(has_include(
            &format!("Include {SSH_DIR}/config.d/auberge.conf"),
            ssh_dir()
        ));
    }

    #[test]
    fn has_include_matches_glob_over_own_config_d() {
        assert!(has_include("Include ~/.ssh/config.d/*.conf", ssh_dir()));
        assert!(has_include("Include ~/.ssh/config.d/*", ssh_dir()));
        assert!(has_include("Include config.d/auberge.*", ssh_dir()));
    }

    #[test]
    fn has_include_rejects_comments_and_other_includes() {
        assert!(!has_include(
            "# Include ~/.ssh/config.d/auberge.conf",
            ssh_dir()
        ));
        assert!(!has_include("Include ~/.ssh/other.conf", ssh_dir()));
        assert!(!has_include("Host auberge\n  HostName 1.2.3.4", ssh_dir()));
        assert!(!has_include("", ssh_dir()));
    }

    #[test]
    fn has_include_rejects_globs_over_foreign_config_d_dirs() {
        assert!(!has_include("Include /etc/ssh/config.d/*", ssh_dir()));
        assert!(!has_include("Include ~/other/config.d/*.conf", ssh_dir()));
        assert!(!has_include("Include ~/.ssh/config.d/*.bak", ssh_dir()));
        assert!(!has_include(
            "Include /etc/ssh/config.d/auberge.conf",
            ssh_dir()
        ));
    }

    #[test]
    fn glob_matches_covers_star_and_question_mark() {
        assert!(glob_matches("auberge.conf", "auberge.conf"));
        assert!(glob_matches("*", "auberge.conf"));
        assert!(glob_matches("*.conf", "auberge.conf"));
        assert!(glob_matches("auberge.*", "auberge.conf"));
        assert!(glob_matches("a?berge.conf", "auberge.conf"));
        assert!(!glob_matches("*.bak", "auberge.conf"));
        assert!(!glob_matches("other.conf", "auberge.conf"));
        assert!(!glob_matches("auberge.conf?", "auberge.conf"));
    }

    #[test]
    fn write_include_file_creates_private_dir_and_file() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");

        let hosts = [fixture_host("auberge", None)];
        let path = write_include_file(&ssh_dir, &hosts).unwrap();

        assert_eq!(path, ssh_dir.join("config.d/auberge.conf"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), render(&hosts));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = std::fs::metadata(ssh_dir.join("config.d"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            let file_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(dir_mode, 0o700);
            assert_eq!(file_mode, 0o600);
        }
    }

    #[test]
    fn write_include_file_overwrites_stale_content() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");

        write_include_file(&ssh_dir, &[fixture_host("old-name", None)]).unwrap();
        let path = write_include_file(&ssh_dir, &[fixture_host("new-name", None)]).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("Host new-name"));
        assert!(!content.contains("old-name"));
    }

    #[test]
    fn main_config_has_include_reads_ssh_dir_config() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        assert!(!main_config_has_include(&ssh_dir).unwrap(), "missing file");

        std::fs::write(ssh_dir.join("config"), "Host x\n  HostName 1.2.3.4\n").unwrap();
        assert!(
            !main_config_has_include(&ssh_dir).unwrap(),
            "no include line"
        );

        std::fs::write(
            ssh_dir.join("config"),
            format!("{INCLUDE_LINE}\n\nHost x\n  HostName 1.2.3.4\n"),
        )
        .unwrap();
        assert!(main_config_has_include(&ssh_dir).unwrap());
    }

    #[test]
    fn main_config_has_include_propagates_non_notfound_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let ssh_dir = tmp.path().join(".ssh");
        std::fs::create_dir_all(ssh_dir.join("config")).unwrap();

        assert!(main_config_has_include(&ssh_dir).is_err());
    }
}
