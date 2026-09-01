//! FreshRSS feed subscriptions, in and out of a Host as an OPML file.
//!
//! These two subcommands are filed under `auberge backup` and are not backup:
//! freshrss's Backup Recipe already carries the app's data directories, so
//! nothing here is what a restore reads. What they are is data portability —
//! moving a feed list between hosts, or in from another reader — and the reason
//! they sat in the backup command is that both reach a Host over ssh, which was
//! the only thing they had in common with it (#673).
//!
//! The CLI surface stays exactly where users and scripts already type it:
//! [`OpmlCommands`] is `#[command(flatten)]`ed into `BackupCommands`, so
//! `auberge backup export-opml` and the `eo`/`io` aliases are unchanged.
//! `src/main.rs` pins that in both directions, because a flatten landing the
//! variants elsewhere still compiles.
//!
//! This is where *auberge* spells the freshrss remote CLI — the install path,
//! the user that runs it, which `cli/*.php` takes which flag. It is not the
//! repo's only spelling of that contract, and the difference is worth knowing
//! before changing either: `ansible/roles/colporteur/tasks/main.yml` runs the
//! same `import-for-user.php` from ansible, as `php <absolute path>` under
//! `become_user` with `--user=` rather than `--user`, against
//! `colporteur_freshrss_install_path` from the role defaults. The tests below
//! pin this side only.

use crate::hosts::{HOST_FLAG, select_or_arg};
use crate::services::ssh::{LiveSshSession, SshSession, resolve_ssh_key_path};
use clap::Subcommand;
use eyre::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub enum OpmlCommands {
    #[command(visible_alias = "eo", about = "Export FreshRSS feeds to OPML file")]
    ExportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Output OPML file path")]
        output: PathBuf,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{host}/{user})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        user: String,
    },
    #[command(visible_alias = "io", about = "Import OPML file to FreshRSS")]
    ImportOpml {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "OPML file to import")]
        input: PathBuf,
        #[arg(
            short = 'k',
            long,
            help = "SSH private key (default: ~/.ssh/identities/{host}/{user})"
        )]
        ssh_key: Option<PathBuf>,
        #[arg(long, default_value = "admin", help = "FreshRSS username")]
        user: String,
    },
}

pub fn run_export_opml(
    host_arg: Option<String>,
    output: PathBuf,
    ssh_key: Option<PathBuf>,
    user: String,
) -> Result<()> {
    let host = select_or_arg(host_arg, HOST_FLAG)?;
    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    eprintln!("Exporting OPML from FreshRSS");
    eprintln!("  Host: {}", host.name);
    eprintln!("  User: {}", user);
    eprintln!("  Output: {}", output.display());

    let route = crate::services::route::resolve(&host, Some(ssh_key_path));
    let session = LiveSshSession::new(&route, &host.become_method)?;
    let opml = export_opml(&session, &user)?;

    fs::write(&output, &opml)
        .wrap_err_with(|| format!("Failed to write OPML to {}", output.display()))?;

    eprintln!("✓ OPML exported successfully");
    eprintln!("  Saved to: {}", output.display());

    Ok(())
}

pub fn run_import_opml(
    host_arg: Option<String>,
    input: PathBuf,
    ssh_key: Option<PathBuf>,
    user: String,
) -> Result<()> {
    let host = select_or_arg(host_arg, HOST_FLAG)?;
    let ssh_key_path = resolve_ssh_key_path(&host, ssh_key)?;
    eprintln!("Using SSH key: {}", ssh_key_path.display());

    if !input.exists() {
        eyre::bail!("OPML file not found: {}", input.display());
    }

    eprintln!("Importing OPML to FreshRSS");
    eprintln!("  Host: {}", host.name);
    eprintln!("  User: {}", user);
    eprintln!("  Input: {}", input.display());

    let route = crate::services::route::resolve(&host, Some(ssh_key_path));
    let session = LiveSshSession::new(&route, &host.become_method)?;
    let stdout = import_opml(&session, &input, &user)?;
    eprintln!("{}", stdout);

    eprintln!("✓ OPML imported successfully");

    Ok(())
}

/// Where the upload lands on the Host. The name carries the user, so imports
/// for different users do not collide — two for the *same* user still do, on a
/// path anyone on the Host can predict. Both were true before this module and
/// are left alone here.
fn staged_path(user: &str) -> String {
    format!("/tmp/freshrss_import_{}.opml", user)
}

fn export_command(user: &str) -> String {
    format!(
        "cd /opt/freshrss && sudo -u freshrss ./cli/export-opml-for-user.php --user {}",
        user
    )
}

/// The staged copy is deleted by the tail of this command rather than a second
/// round trip, and `&&`-chained — so a failed import leaves the file on the
/// Host to look at. What makes the next attempt safe is not this `rm` but scp
/// truncating the destination.
fn import_command(user: &str, staged: &str) -> String {
    format!(
        "cd /opt/freshrss && sudo -u freshrss ./cli/import-for-user.php --user {} --filename {} && rm {}",
        user, staged, staged
    )
}

/// FreshRSS's own export CLI writes the OPML to stdout, so the export is the
/// captured bytes of one remote command.
fn export_opml(session: &dyn SshSession, user: &str) -> Result<Vec<u8>> {
    let out = session.run(&export_command(user))?;
    if !out.success {
        eyre::bail!("OPML export failed: {}", out.stderr_str());
    }
    Ok(out.stdout)
}

/// Upload, then import.
///
/// The staged path and the command that consumes it are both derived from
/// `user` here rather than passed in, so they cannot disagree about which file
/// on the Host is being imported.
fn import_opml(session: &dyn SshSession, local: &Path, user: &str) -> Result<String> {
    let staged = staged_path(user);

    eprintln!("  Uploading OPML file...");
    session
        .scp_to(local, &staged)
        .wrap_err("Failed to upload OPML file")?;

    eprintln!("  Importing feeds...");
    let out = session
        .run(&import_command(user, &staged))
        .wrap_err("Failed to execute import command")?;
    if !out.success {
        eyre::bail!("OPML import failed: {}", out.stderr_str());
    }
    Ok(out.stdout_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::ssh::{CommandResult, MockSshSession, SshOp};

    #[test]
    fn export_opml_returns_the_remote_stdout_verbatim() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout("<opml version=\"2.0\"/>"));
        assert_eq!(
            export_opml(&mock, "admin").unwrap(),
            b"<opml version=\"2.0\"/>".to_vec()
        );
        assert_eq!(mock.calls(), vec![SshOp::Run(export_command("admin"))]);
    }

    #[test]
    fn export_opml_reports_the_remote_stderr() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stderr("User 'ghost' does not exist"));
        let err = export_opml(&mock, "ghost").unwrap_err();
        assert_eq!(
            err.to_string(),
            "OPML export failed: User 'ghost' does not exist"
        );
    }

    /// Also that both ops name the same staged file. They used to be separate
    /// parameters, so a caller could scp to one path and import another.
    #[test]
    fn import_opml_uploads_before_it_imports() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stdout("12 feeds imported\n"));
        let out = import_opml(&mock, Path::new("/tmp/feeds.opml"), "me").unwrap();
        assert_eq!(out, "12 feeds imported\n");
        assert_eq!(
            mock.calls(),
            vec![
                SshOp::ScpTo {
                    local: PathBuf::from("/tmp/feeds.opml"),
                    remote: "/tmp/freshrss_import_me.opml".to_string(),
                },
                SshOp::Run(import_command("me", "/tmp/freshrss_import_me.opml")),
            ]
        );
    }

    #[test]
    fn import_opml_reports_the_remote_stderr() {
        let mock = MockSshSession::new();
        mock.stage_run_result(CommandResult::from_stderr("malformed OPML"));
        let err = import_opml(&mock, Path::new("/tmp/feeds.opml"), "me").unwrap_err();
        assert_eq!(err.to_string(), "OPML import failed: malformed OPML");
    }

    /// The remote command lines are the whole of this module's freshrss
    /// knowledge, and they used to be built inline in the backup command where
    /// nothing read them. Pinned as text because text is what the Host runs.
    #[test]
    fn the_remote_commands_name_the_freshrss_cli() {
        assert_eq!(
            export_command("admin"),
            "cd /opt/freshrss && sudo -u freshrss ./cli/export-opml-for-user.php --user admin"
        );
        assert_eq!(staged_path("admin"), "/tmp/freshrss_import_admin.opml");
        assert_eq!(
            import_command("admin", "/tmp/freshrss_import_admin.opml"),
            "cd /opt/freshrss && sudo -u freshrss ./cli/import-for-user.php --user admin \
             --filename /tmp/freshrss_import_admin.opml && rm /tmp/freshrss_import_admin.opml"
        );
    }
}
