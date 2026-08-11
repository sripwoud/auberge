mod reconcile;
mod rescan;
mod verify;

use crate::output::OutputFormat;
use crate::services::bichon::rescan::ARCHIVE_DIR;
use clap::Subcommand;
use eyre::Result;

pub use reconcile::run_reconcile_folders;
pub use rescan::run_rescan;
pub use verify::run_verify_coverage;

#[derive(Subcommand)]
pub enum BichonCommands {
    #[command(
        alias = "rf",
        about = "Reconcile account sync_folders from live IMAP folders"
    )]
    ReconcileFolders {
        #[arg(short = 'H', long, help = "Target host running Bichon")]
        host: String,
        #[arg(long, help = "Apply changes to Bichon accounts")]
        apply: bool,
        #[arg(long, help = "Only reconcile one account email")]
        account: Option<String>,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
    #[command(
        alias = "vc",
        about = "Verify one folder's Email Archive coverage by message identity"
    )]
    VerifyCoverage {
        #[arg(short = 'H', long, help = "Target host running Bichon")]
        host: String,
        #[arg(
            long,
            help = "Account email; it also names the Email Archive directory"
        )]
        account: String,
        #[arg(long, help = "Folder whose coverage to verify")]
        folder: String,
        #[arg(
            long,
            value_name = "YYYY-MM-DD",
            help = "Verify messages dated strictly before this UTC date"
        )]
        before: String,
        #[arg(
            long,
            default_value = ARCHIVE_DIR,
            help = "Email Archive root on the host"
        )]
        archive_path: String,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
    #[command(
        alias = "rs",
        about = "Re-archive mail whose Date predates the archive cursor"
    )]
    Rescan {
        #[arg(
            short = 'H',
            long,
            help = "Target host running Bichon (prompted on a TTY when omitted)"
        )]
        host: Option<String>,
        #[arg(
            long,
            help = "Only rescan one account email (prompted on a TTY when omitted)"
        )]
        account: Option<String>,
        #[arg(
            short = 'o',
            long,
            value_enum,
            default_value = "human",
            help = "Output format"
        )]
        output: OutputFormat,
    },
}

pub async fn run_bichon_command(cmd: BichonCommands) -> Result<i32> {
    match cmd {
        BichonCommands::ReconcileFolders {
            host,
            apply,
            account,
            output,
        } => {
            run_reconcile_folders(host, apply, account, output).await?;
            Ok(0)
        }
        BichonCommands::VerifyCoverage {
            host,
            account,
            folder,
            before,
            archive_path,
            output,
        } => run_verify_coverage(host, account, folder, before, archive_path, output).await,
        BichonCommands::Rescan {
            host,
            account,
            output,
        } => run_rescan(host, account, output).await,
    }
}
