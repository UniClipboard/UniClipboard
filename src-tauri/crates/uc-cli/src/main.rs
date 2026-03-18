mod commands;
mod exit_codes;
mod output;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "uniclipboard-cli",
    version,
    about = "UniClipboard command-line interface"
)]
struct Cli {
    /// Output in JSON format
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show daemon status
    Status,
    /// List paired devices (direct mode, no daemon required)
    Devices,
    /// Show space and encryption status (direct mode, no daemon required)
    SpaceStatus,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let exit_code = rt.block_on(async {
        match cli.command {
            Commands::Status => commands::status::run(cli.json).await,
            Commands::Devices => commands::devices::run(cli.json).await,
            Commands::SpaceStatus => commands::space_status::run(cli.json).await,
        }
    });

    std::process::exit(exit_code);
}
