mod reconcile;

use crate::output::OutputFormat;
use clap::Subcommand;
use eyre::Result;

pub use reconcile::run_reconcile_folders;

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
}

pub async fn run_bichon_command(cmd: BichonCommands) -> Result<()> {
    match cmd {
        BichonCommands::ReconcileFolders {
            host,
            apply,
            account,
            output,
        } => run_reconcile_folders(host, apply, account, output).await,
    }
}
