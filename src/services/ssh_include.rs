use crate::hosts::Host;
use eyre::{Context, Result};
use std::path::{Path, PathBuf};

pub const INCLUDE_LINE: &str = "Include ~/.ssh/config.d/auberge.conf";
const RELATIVE_PATH: &str = "config.d/auberge.conf";

pub fn include_file_path(ssh_dir: &Path) -> PathBuf {
    ssh_dir.join(RELATIVE_PATH)
}

pub fn render(hosts: &[Host]) -> String {
    let mut out = String::from(
        "# Managed by auberge. Regenerated from hosts.toml on every\n\
         # `auberge host add|edit|rename|remove` - do not edit by hand.\n",
    );
    for host in hosts {
        out.push_str(&format!(
            "\nHost {}\n  HostName {}\n  Port {}\n  User {}\n  IdentityFile {}\n  IdentitiesOnly yes\n  StrictHostKeyChecking accept-new\n",
            host.name,
            host.address,
            host.port,
            host.user,
            identity_file(host)
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

pub fn main_config_has_include(ssh_dir: &Path) -> bool {
    std::fs::read_to_string(ssh_dir.join("config"))
        .map(|content| has_include(&content))
        .unwrap_or(false)
}

/// A glob include (`Include ~/.ssh/config.d/*.conf`) also loads the auberge
/// file, so it counts: a false "missing" would nag on every host subcommand.
pub fn has_include(main_config: &str) -> bool {
    main_config.lines().any(|line| {
        let mut tokens = line.split_whitespace();
        let Some(directive) = tokens.next() else {
            return false;
        };
        if !directive.eq_ignore_ascii_case("include") {
            return false;
        }
        tokens.any(|arg| {
            let arg = arg.trim_matches('"');
            arg.ends_with(RELATIVE_PATH) || (arg.contains("config.d/") && arg.contains('*'))
        })
    })
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

    #[test]
    fn has_include_matches_exact_line() {
        assert!(has_include(INCLUDE_LINE));
        assert!(has_include("  Include ~/.ssh/config.d/auberge.conf"));
        assert!(has_include("include ~/.ssh/config.d/auberge.conf"));
        assert!(has_include("Include \"~/.ssh/config.d/auberge.conf\""));
        assert!(has_include("Include config.d/auberge.conf"));
    }

    #[test]
    fn has_include_matches_glob_over_config_d() {
        assert!(has_include("Include ~/.ssh/config.d/*.conf"));
        assert!(has_include("Include ~/.ssh/config.d/*"));
    }

    #[test]
    fn has_include_rejects_comments_and_other_includes() {
        assert!(!has_include("# Include ~/.ssh/config.d/auberge.conf"));
        assert!(!has_include("Include ~/.ssh/other.conf"));
        assert!(!has_include("Host auberge\n  HostName 1.2.3.4"));
        assert!(!has_include(""));
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

        assert!(!main_config_has_include(&ssh_dir), "missing file");

        std::fs::write(ssh_dir.join("config"), "Host x\n  HostName 1.2.3.4\n").unwrap();
        assert!(!main_config_has_include(&ssh_dir), "no include line");

        std::fs::write(
            ssh_dir.join("config"),
            format!("{INCLUDE_LINE}\n\nHost x\n  HostName 1.2.3.4\n"),
        )
        .unwrap();
        assert!(main_config_has_include(&ssh_dir));
    }
}
