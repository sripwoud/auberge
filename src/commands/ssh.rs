use crate::hosts::HOST_FLAG;
use crate::output;
use crate::prompt::{Choice, select_item};
use crate::services::inventory::select_or_arg as inventory_select_or_arg;
use crate::services::ssh::{LiveSshSession, SshSession};
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::os::unix::fs::DirBuilderExt;
use std::process::Command;

#[derive(Subcommand)]
pub enum SshCommands {
    #[command(
        visible_alias = "k",
        about = "Generate an ed25519 SSH identity for a host"
    )]
    Keygen {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            default_value = "ansible",
            help = "User (ansible or your configured username)"
        )]
        user: String,
        #[arg(short, long, help = "Force overwrite existing key")]
        force: bool,
    },
    #[command(
        visible_alias = "ak",
        about = "Add/authorize SSH public key on remote host"
    )]
    AddKey {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short = 'c', long, help = "SSH private key to connect with")]
        connect_with: Option<std::path::PathBuf>,
        #[arg(short = 'a', long, help = "Public key file to authorize on remote")]
        authorize: Option<std::path::PathBuf>,
        #[arg(
            short,
            long,
            default_value = "ansible",
            help = "Remote user to authorize key for"
        )]
        user: String,
        #[arg(short = 'y', long, help = "Skip confirmation prompt")]
        yes: bool,
    },
}

pub fn run_ssh_keygen(host_arg: Option<String>, user: String, force: bool) -> Result<()> {
    let host = inventory_select_or_arg(host_arg, HOST_FLAG)?;

    let key_path = crate::services::ssh::default_ssh_key_path(&user, &host.name)?;

    if key_path.exists() && !force {
        output::success(&format!("Key already exists: {}", key_path.display()));
        return Ok(());
    }

    let host_dir = key_path
        .parent()
        .expect("derived key path always has a parent");

    let legacy_path = crate::services::ssh::legacy_ssh_key_path(&user, &host.name)?;
    if !force && legacy_path.exists() {
        eyre::bail!(
            "Found key at legacy path: {}\nMigrate it: mkdir -p {} && mv {} {} && mv {}.pub {}.pub\nOr re-run with --force to generate a fresh key instead",
            legacy_path.display(),
            host_dir.display(),
            legacy_path.display(),
            key_path.display(),
            legacy_path.display(),
            key_path.display()
        );
    }

    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(host_dir)
        .wrap_err("Failed to create SSH identities directory")?;

    if force && key_path.exists() {
        std::fs::remove_file(&key_path).wrap_err("Failed to remove existing key")?;
        let pub_path = std::path::PathBuf::from(format!("{}.pub", key_path.display()));
        if pub_path.exists() {
            std::fs::remove_file(&pub_path).wrap_err("Failed to remove existing public key")?;
        }
    }

    output::info(&format!("Generating SSH key for {}@{}", user, host.name));

    let mut cmd = Command::new("ssh-keygen");
    cmd.arg("-t")
        .arg("ed25519")
        .arg("-f")
        .arg(&key_path)
        .arg("-C")
        .arg(format!("{}@{}", user, host.name))
        .arg("-N")
        .arg("");

    let result =
        output::run_piped("ssh-keygen", &mut cmd).wrap_err("Failed to execute ssh-keygen")?;
    if result.status.success() {
        output::clear_subprocess_lines(result.lines_written);
        output::success(&format!("Generated key: {}", key_path.display()));
        output::info(&format!("Public key: {}.pub", key_path.display()));
        Ok(())
    } else {
        Err(result.error("ssh-keygen failed"))
    }
}

pub fn run_ssh_add_key(
    host_arg: Option<String>,
    connect_with: Option<std::path::PathBuf>,
    authorize: Option<std::path::PathBuf>,
    user: String,
    yes: bool,
) -> Result<()> {
    let host = inventory_select_or_arg(host_arg, HOST_FLAG)?;

    let home_dir =
        dirs::home_dir().ok_or_else(|| eyre::eyre!("Could not determine home directory"))?;

    let connect_key = match connect_with {
        Some(path) => path,
        None => {
            let default_key = crate::services::ssh::default_ssh_key_path(&user, &host.name)?;

            if default_key.exists() {
                output::info(&format!(
                    "Using default connection key: {}",
                    default_key.display()
                ));
                default_key
            } else {
                let available_keys = scan_private_keys(&home_dir, &host.name)?;
                if available_keys.is_empty() {
                    eyre::bail!(
                        "No SSH private keys found. Generate one with 'auberge ssh keygen'"
                    );
                }

                select_item(
                    &available_keys,
                    |path| path.display().to_string(),
                    Choice::new("SSH key")
                        .with_prompt("Select SSH key to connect with")
                        .resolved_by("-c <key>"),
                )?
            }
        }
    };

    if !connect_key.exists() {
        eyre::bail!("Connection key not found: {}", connect_key.display());
    }

    let pubkey_to_authorize = match authorize {
        Some(path) => path,
        None => {
            let available_pubkeys = scan_public_keys(&home_dir, &host.name)?;
            if available_pubkeys.is_empty() {
                eyre::bail!("No SSH public keys found. Generate one with 'auberge ssh keygen'");
            }

            select_item(
                &available_pubkeys,
                |path| path.display().to_string(),
                Choice::new("public key")
                    .with_prompt("Select public key to authorize on remote")
                    .resolved_by("-a <key>"),
            )?
        }
    };

    if !pubkey_to_authorize.exists() {
        eyre::bail!("Public key not found: {}", pubkey_to_authorize.display());
    }

    let pubkey_content = std::fs::read_to_string(&pubkey_to_authorize).wrap_err_with(|| {
        format!(
            "Failed to read public key: {}",
            pubkey_to_authorize.display()
        )
    })?;

    output::info("Add SSH Key");
    output::info(&format!(
        "Host: {} ({}:{})",
        host.name, host.vars.ansible_host, host.vars.ansible_port
    ));
    output::info(&format!("Remote user: {}", user));
    output::info(&format!("Connection key: {}", connect_key.display()));
    output::info(&format!(
        "Key to authorize: {}",
        pubkey_to_authorize.display()
    ));
    output::info(&format!("Public key preview: {}", pubkey_content.trim()));

    if !crate::prompt::confirm("Authorize this key on the remote host?", yes) {
        eprintln!("Cancelled.");
        return Ok(());
    }

    output::info("Adding key to remote host");

    let ssh_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && echo 'Key added successfully'",
        pubkey_content.trim()
    );

    let target = host.ssh_target(&user);
    let result = LiveSshSession::new(&target, &connect_key)
        .run(&ssh_cmd)
        .wrap_err("Failed to execute SSH command")?;

    if !result.success {
        let stderr = result.stderr_str();
        let stderr = stderr.trim();
        if stderr.is_empty() {
            eyre::bail!("Failed to add key to remote host");
        }
        eyre::bail!("Failed to add key to remote host: {}", stderr);
    }

    output::success(&format!(
        "Key authorized successfully on {}@{}",
        user, host.name
    ));
    Ok(())
}

fn sorted_key_files(
    dir: &std::path::Path,
    is_match: impl Fn(&std::path::Path) -> bool,
) -> Result<Vec<std::path::PathBuf>> {
    let mut keys = Vec::new();
    if !dir.is_dir() {
        return Ok(keys);
    }
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_file() && is_match(&path) {
            keys.push(path);
        }
    }
    keys.sort();
    Ok(keys)
}

fn scan_private_keys(
    home_dir: &std::path::Path,
    host_name: &str,
) -> Result<Vec<std::path::PathBuf>> {
    let is_private = |path: &std::path::Path| path.extension().is_none_or(|ext| ext != "pub");

    let mut keys = Vec::new();
    for dir in crate::services::ssh::identity_scan_dirs(home_dir, host_name) {
        keys.extend(sorted_key_files(&dir, is_private)?);
    }
    keys.extend(sorted_key_files(&home_dir.join(".ssh"), |path| {
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        is_private(path) && (file_name.starts_with("id_") || file_name == "identity")
    })?);
    Ok(keys)
}

fn scan_public_keys(
    home_dir: &std::path::Path,
    host_name: &str,
) -> Result<Vec<std::path::PathBuf>> {
    let is_public = |path: &std::path::Path| path.extension().is_some_and(|ext| ext == "pub");

    let mut keys = Vec::new();
    for dir in crate::services::ssh::identity_scan_dirs(home_dir, host_name) {
        keys.extend(sorted_key_files(&dir, is_public)?);
    }
    keys.extend(sorted_key_files(&home_dir.join(".ssh"), is_public)?);
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_file(path: &std::path::Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn test_scan_private_keys_includes_host_subdir_and_flat_service_keys() {
        let home = tempfile::tempdir().unwrap();
        let identities = home.path().join(".ssh/identities");
        write_file(&identities.join("myserver/ansible"), "key");
        write_file(&identities.join("myserver/ansible.pub"), "pub");
        write_file(&identities.join("github"), "key");
        write_file(&identities.join("other-host/ansible"), "key");

        let keys = scan_private_keys(home.path(), "myserver").unwrap();

        assert!(keys.contains(&identities.join("myserver/ansible")));
        assert!(keys.contains(&identities.join("github")));
        assert!(!keys.contains(&identities.join("myserver/ansible.pub")));
        assert!(!keys.contains(&identities.join("other-host/ansible")));
    }

    #[test]
    fn test_scan_public_keys_includes_host_subdir_and_flat_service_keys() {
        let home = tempfile::tempdir().unwrap();
        let identities = home.path().join(".ssh/identities");
        write_file(&identities.join("myserver/ansible"), "key");
        write_file(&identities.join("myserver/ansible.pub"), "pub");
        write_file(&identities.join("github.pub"), "pub");

        let keys = scan_public_keys(home.path(), "myserver").unwrap();

        assert!(keys.contains(&identities.join("myserver/ansible.pub")));
        assert!(keys.contains(&identities.join("github.pub")));
        assert!(!keys.contains(&identities.join("myserver/ansible")));
    }

    #[test]
    fn test_scan_private_keys_lists_host_subdir_keys_first() {
        let home = tempfile::tempdir().unwrap();
        let identities = home.path().join(".ssh/identities");
        write_file(&home.path().join(".ssh/id_ed25519"), "key");
        write_file(&identities.join("aaa-service"), "key");
        write_file(&identities.join("myserver/ansible"), "key");

        let keys = scan_private_keys(home.path(), "myserver").unwrap();

        assert_eq!(
            keys,
            vec![
                identities.join("myserver/ansible"),
                identities.join("aaa-service"),
                home.path().join(".ssh/id_ed25519"),
            ]
        );
    }

    #[test]
    fn test_scan_skips_host_subdir_itself_as_flat_key() {
        let home = tempfile::tempdir().unwrap();
        let identities = home.path().join(".ssh/identities");
        write_file(&identities.join("myserver/ansible"), "key");

        let keys = scan_private_keys(home.path(), "myserver").unwrap();

        assert!(!keys.contains(&identities.join("myserver")));
    }
}
