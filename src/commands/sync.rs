use crate::hosts::{HOST_FLAG, HostManager, select_or_arg as hosts_select_or_arg};
use crate::output;
use crate::services::inventory::select_or_arg as inventory_select_or_arg;
use crate::services::progress::{Progress, TerminalProgress};
use crate::services::rsync::{parse_rsync_progress, parse_transferred_size};
use crate::ssh_session::SshSession;
use clap::Subcommand;
use eyre::{Result, WrapErr};
use std::path::{Path, PathBuf};
use std::process::Command;

// `--info=progress2` reports one running byte count for the whole transfer
// rather than per-file lines, which is why `-v` and `-P`'s `--progress` are
// gone and `--partial` is spelled out. `--no-inc-recursive` builds the file
// list up front so the count cannot run backwards mid-sync.
const MUSIC_RSYNC_FLAGS: [&str; 5] = [
    "-rltz",
    "--partial",
    "--info=progress2",
    "--no-inc-recursive",
    "--omit-dir-times",
];

#[derive(Subcommand)]
pub enum SyncCommands {
    #[command(
        visible_alias = "m",
        about = "Sync a local music library to the host's Navidrome directory"
    )]
    Music {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(short, long, help = "Source music directory [default: ~/Music]")]
        source: Option<PathBuf>,
        #[arg(short = 'n', long, help = "Dry run (don't actually sync)")]
        dry_run: bool,
    },
    #[command(visible_alias = "h", about = "Sync hermes config and restart service")]
    Hermes {
        #[arg(short = 'H', long, help = "Target host")]
        host: Option<String>,
        #[arg(
            short,
            long,
            help = "Config file path: source when pushing, destination when pulling [default: ~/.config/hermes/config.yaml]"
        )]
        source: Option<PathBuf>,
        #[arg(
            short = 'n',
            long,
            help = "Dry run (don't actually sync)",
            conflicts_with = "pull"
        )]
        dry_run: bool,
        #[arg(
            short = 'p',
            long,
            help = "Pull config from remote to local instead of pushing"
        )]
        pull: bool,
    },
}

// One builder for both passes: the scan's byte count is only a valid
// denominator for the transfer if every file-selecting flag matches.
fn music_rsync_command(source: &Path, ssh_arg: &str, destination: &str) -> Command {
    let mut cmd = Command::new("rsync");
    cmd.args(MUSIC_RSYNC_FLAGS)
        .arg("--delete")
        .arg("--exclude=.DS_Store")
        .arg("--exclude=*.tmp")
        .arg("-e")
        .arg(ssh_arg)
        .arg(format!("{}/", source.display()))
        .arg(destination);
    cmd
}

fn music_scan_command(source: &Path, ssh_arg: &str, destination: &str) -> Command {
    let mut cmd = music_rsync_command(source, ssh_arg, destination);
    cmd.arg("--dry-run").arg("--stats");
    cmd
}

fn scan_transfer_size(cmd: &mut Command) -> Result<u64> {
    let scan = cmd.output().wrap_err("Failed to execute rsync")?;
    if !scan.status.success() {
        eyre::bail!(
            "rsync scan failed: {}",
            String::from_utf8_lossy(&scan.stderr).trim()
        );
    }
    let stats = String::from_utf8_lossy(&scan.stdout);
    parse_transferred_size(&stats)
        .ok_or_else(|| eyre::eyre!("rsync --stats reported no transferred file size"))
}

fn drive_music_sync(
    dry_run: bool,
    progress: &mut dyn Progress,
    scan: impl FnOnce() -> Result<u64>,
    transfer: impl FnOnce(&mut dyn Progress) -> Result<()>,
) -> Result<()> {
    progress.task_started("Scanning music library");
    let total = scan().inspect_err(|_| progress.task_done())?;

    if dry_run {
        progress.info(&format!("Would transfer {}", output::format_size(total)));
        progress.task_done();
        return Ok(());
    }

    progress.task_started("Syncing music");
    // Nothing to send leaves the spinner up rather than parking a 0/0 bar.
    progress.set_total((total > 0).then_some(total));

    let result = transfer(progress);
    progress.task_done();
    result
}

pub fn run_sync_music(
    host_arg: Option<String>,
    source: Option<PathBuf>,
    dry_run: bool,
) -> Result<()> {
    let ansible_user = "ansible";

    let host = inventory_select_or_arg(host_arg, HOST_FLAG)?;

    let music_source = source.unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join("Music"))
            .unwrap_or_else(|| PathBuf::from("~/Music"))
    });

    if !music_source.exists() {
        eyre::bail!(
            "Music source directory not found: {}",
            music_source.display()
        );
    }

    let ssh_key = dirs::home_dir()
        .ok_or_else(|| eyre::eyre!("Could not determine home directory"))?
        .join(format!(".ssh/identities/{}_{}", ansible_user, host.name));

    if !ssh_key.exists() {
        eyre::bail!(
            "SSH key not found: {}\nRun 'auberge ssh keygen --host {} --user {}' first",
            ssh_key.display(),
            host.name,
            ansible_user
        );
    }

    let remote_path = "/srv/music/";
    let ssh_arg = format!("ssh -p {} -i {}", host.vars.ansible_port, ssh_key.display());
    let destination = format!(
        "{}@{}:{}",
        ansible_user, host.vars.ansible_host, remote_path
    );

    output::info(&format!("Syncing music to {}", destination));
    if dry_run {
        output::info("Dry run mode");
    }

    let mut progress = TerminalProgress::new("");
    drive_music_sync(
        dry_run,
        &mut progress,
        || {
            scan_transfer_size(&mut music_scan_command(
                &music_source,
                &ssh_arg,
                &destination,
            ))
        },
        |progress| {
            let mut cmd = music_rsync_command(&music_source, &ssh_arg, &destination);
            let stream = output::stream_command_segments("rsync", &mut cmd, |segment| {
                if let Some(reported) = parse_rsync_progress(segment) {
                    progress.bytes_transferred(reported.bytes_transferred);
                }
            })
            .wrap_err("Failed to execute rsync")?;

            if !stream.status.success() {
                let tail = stream.last_stderr.trim();
                if tail.is_empty() {
                    eyre::bail!("rsync failed");
                }
                eyre::bail!("rsync failed: {}", tail);
            }
            Ok(())
        },
    )?;

    if !dry_run {
        output::success("Music sync completed");
    }
    Ok(())
}

pub fn run_sync_hermes(
    host_arg: Option<String>,
    source: Option<PathBuf>,
    dry_run: bool,
    pull: bool,
) -> Result<()> {
    let xdg_host = match host_arg {
        Some(name) => HostManager::get_host(&name)?,
        None => hosts_select_or_arg(None, HOST_FLAG)?,
    };

    if pull {
        let local_dest = match source {
            Some(s) => s,
            None => dirs::home_dir()
                .map(|h| h.join(".config/hermes/config.yaml"))
                .ok_or_else(|| {
                    eyre::eyre!("Could not determine home directory for Hermes config")
                })?,
        };
        let ssh_key = xdg_host
            .ssh_key
            .as_ref()
            .map(PathBuf::from)
            .ok_or_else(|| eyre::eyre!("No SSH key configured for host '{}'", xdg_host.name))?;
        if !ssh_key.exists() {
            eyre::bail!("SSH key not found: {}", ssh_key.display());
        }
        if let Some(parent) = local_dest.parent() {
            std::fs::create_dir_all(parent)
                .wrap_err_with(|| format!("Failed to create directory: {}", parent.display()))?;
        }
        let session = SshSession::new(&xdg_host, &ssh_key);
        output::info(&format!(
            "Pulling hermes config from remote to {}",
            local_dest.display()
        ));
        session.scp_from(".hermes/config.yaml", &local_dest)?;
        output::success("Hermes config pulled");
        return Ok(());
    }

    let config_source = match source {
        Some(s) => s,
        None => dirs::home_dir()
            .map(|h| h.join(".config/hermes/config.yaml"))
            .ok_or_else(|| eyre::eyre!("Could not determine home directory for Hermes config"))?,
    };

    if !config_source.exists() {
        eyre::bail!(
            "Hermes config not found: {}\nCreate it at ~/.config/hermes/config.yaml first",
            config_source.display()
        );
    }

    let ssh_key = xdg_host
        .ssh_key
        .as_ref()
        .map(PathBuf::from)
        .ok_or_else(|| eyre::eyre!("No SSH key configured for host '{}'", xdg_host.name))?;

    if !ssh_key.exists() {
        eyre::bail!("SSH key not found: {}", ssh_key.display());
    }

    let session = SshSession::new(&xdg_host, &ssh_key);
    let remote_dest = format!("{}@{}:.hermes/config.yaml", xdg_host.user, xdg_host.address);

    output::info("Preparing remote ~/.hermes directory...");
    let prepare = session
        .run("mkdir -p ~/.hermes")
        .wrap_err("Failed to prepare remote ~/.hermes directory")?;
    if !prepare.status.success() {
        eyre::bail!("Remote ~/.hermes directory is missing and could not be created");
    }

    output::info(&format!("Syncing hermes config to {}", remote_dest));

    let mut cmd = Command::new("rsync");
    cmd.arg("-az")
        .arg("-e")
        .arg(session.rsync_e_arg())
        .arg(&config_source)
        .arg(&remote_dest);

    if dry_run {
        cmd.arg("--dry-run");
        output::info("Dry run mode");
    }

    let result = output::run_piped("rsync", &mut cmd).wrap_err("Failed to execute rsync")?;
    if !result.status.success() {
        return Err(result.error("rsync failed"));
    }
    output::clear_subprocess_lines(result.lines_written);
    output::success("Hermes config synced");

    if dry_run {
        return Ok(());
    }

    output::info("Restarting hermes-gateway...");
    let restart = session
        .run("XDG_RUNTIME_DIR=/run/user/$(id -u) systemctl --user restart hermes-gateway")
        .wrap_err("Failed to restart hermes-gateway")?;
    if !restart.status.success() {
        eyre::bail!("hermes-gateway restart failed");
    }
    output::success("hermes-gateway restarted");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::progress::{MockProgress, ProgressEvent};

    fn short_flag_letters() -> String {
        MUSIC_RSYNC_FLAGS
            .iter()
            .filter(|f| !f.starts_with("--"))
            .flat_map(|f| f.trim_start_matches('-').chars())
            .collect()
    }

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn transfer_command() -> Command {
        music_rsync_command(
            Path::new("/home/u/Music"),
            "ssh -p 22",
            "ansible@h:/srv/music/",
        )
    }

    fn scan_command() -> Command {
        music_scan_command(
            Path::new("/home/u/Music"),
            "ssh -p 22",
            "ansible@h:/srv/music/",
        )
    }

    #[test]
    fn music_rsync_omits_dir_times() {
        assert!(MUSIC_RSYNC_FLAGS.contains(&"--omit-dir-times"));
    }

    #[test]
    fn music_rsync_requests_progress2() {
        assert!(MUSIC_RSYNC_FLAGS.contains(&"--info=progress2"));
    }

    #[test]
    fn music_rsync_disables_incremental_recursion() {
        assert!(MUSIC_RSYNC_FLAGS.contains(&"--no-inc-recursive"));
    }

    #[test]
    fn music_rsync_retains_partial_after_dropping_capital_p() {
        assert!(MUSIC_RSYNC_FLAGS.contains(&"--partial"));
        assert!(!short_flag_letters().contains('P'));
    }

    #[test]
    fn music_rsync_drops_verbose_now_that_progress2_reports() {
        assert!(!short_flag_letters().contains('v'));
    }

    #[test]
    fn transfer_command_carries_no_dry_run() {
        assert!(
            !args_of(&transfer_command())
                .iter()
                .any(|a| a == "--dry-run")
        );
    }

    #[test]
    fn scan_command_adds_dry_run_and_stats() {
        let args = args_of(&scan_command());
        assert!(args.contains(&"--dry-run".to_string()));
        assert!(args.contains(&"--stats".to_string()));
    }

    // A scan that selects different files than the transfer yields a
    // denominator the bar can never reach.
    #[test]
    fn scan_command_is_the_transfer_command_plus_two_flags() {
        let transfer = args_of(&transfer_command());
        let scan = args_of(&scan_command());
        assert_eq!(scan[..transfer.len()], transfer[..]);
        assert_eq!(&scan[transfer.len()..], ["--dry-run", "--stats"]);
    }

    #[test]
    fn both_commands_delete_and_exclude_identically() {
        for args in [args_of(&transfer_command()), args_of(&scan_command())] {
            assert!(args.contains(&"--delete".to_string()));
            assert!(args.contains(&"--exclude=.DS_Store".to_string()));
            assert!(args.contains(&"--exclude=*.tmp".to_string()));
        }
    }

    #[test]
    fn dry_run_scans_without_transferring() {
        let mut progress = MockProgress::new();
        let mut transferred = false;
        drive_music_sync(
            true,
            &mut progress,
            || Ok(9_000),
            |_| {
                transferred = true;
                Ok(())
            },
        )
        .unwrap();

        assert!(!transferred, "dry run must not spawn the transfer");
        assert!(
            progress
                .events()
                .iter()
                .any(|e| matches!(e, ProgressEvent::Info(msg) if msg.contains("Would transfer")))
        );
    }

    #[test]
    fn nothing_to_transfer_leaves_the_spinner_instead_of_a_zero_bar() {
        let mut progress = MockProgress::new();
        drive_music_sync(false, &mut progress, || Ok(0), |_| Ok(())).unwrap();

        assert!(progress.events().contains(&ProgressEvent::SetTotal(None)));
    }

    #[test]
    fn scanned_total_becomes_the_bar_length_before_transferring() {
        let mut progress = MockProgress::new();
        drive_music_sync(
            false,
            &mut progress,
            || Ok(9_300_000_000),
            |p| {
                p.bytes_transferred(1_200_000_000);
                Ok(())
            },
        )
        .unwrap();

        assert_eq!(
            progress.events(),
            &[
                ProgressEvent::TaskStarted("Scanning music library".to_string()),
                ProgressEvent::TaskStarted("Syncing music".to_string()),
                ProgressEvent::SetTotal(Some(9_300_000_000)),
                ProgressEvent::BytesTransferred(1_200_000_000),
                ProgressEvent::TaskDone,
            ]
        );
    }

    #[test]
    fn failed_scan_clears_the_spinner_and_skips_the_transfer() {
        let mut progress = MockProgress::new();
        let mut transferred = false;
        let result = drive_music_sync(
            false,
            &mut progress,
            || eyre::bail!("rsync scan failed: permission denied"),
            |_| {
                transferred = true;
                Ok(())
            },
        );

        assert!(result.is_err());
        assert!(!transferred);
        assert_eq!(progress.events().last(), Some(&ProgressEvent::TaskDone));
    }

    #[test]
    fn failed_transfer_still_clears_the_bar() {
        let mut progress = MockProgress::new();
        let result = drive_music_sync(
            false,
            &mut progress,
            || Ok(1_024),
            |_| eyre::bail!("rsync failed: connection reset"),
        );

        assert!(result.is_err());
        assert_eq!(progress.events().last(), Some(&ProgressEvent::TaskDone));
    }

    #[test]
    fn music_rsync_does_not_use_archive_mode() {
        assert!(!short_flag_letters().contains('a'));
    }

    #[test]
    fn music_rsync_does_not_preserve_permissions() {
        assert!(!short_flag_letters().contains('p'));
    }

    #[test]
    fn music_rsync_preserves_file_times() {
        assert!(short_flag_letters().contains('t'));
    }

    #[test]
    fn music_rsync_recurses() {
        assert!(short_flag_letters().contains('r'));
    }
}
