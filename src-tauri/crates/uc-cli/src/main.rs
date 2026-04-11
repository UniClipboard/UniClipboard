mod autostop;
mod commands;
mod exit_codes;
mod local_daemon;
mod output;
mod ui;

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

    /// Enable verbose tracing output (shows debug logs on console)
    #[arg(long, short, global = true)]
    verbose: bool,

    /// Run in development mode (use file-based secure storage instead of system keychain)
    #[cfg_attr(not(debug_assertions), arg(long, global = true, hide = true))]
    #[cfg_attr(debug_assertions, arg(long, global = true))]
    dev: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the daemon (background by default, use --foreground for log streaming)
    Start {
        /// Run daemon in foreground (log output to terminal)
        #[arg(
            long,
            short = 'f',
            help = "Run daemon in foreground (log output to terminal)"
        )]
        foreground: bool,
    },
    /// Stop the running daemon
    Stop,
    /// Show daemon status
    Status,
    /// Drive daemon-owned setup flows (interactive guide when no subcommand given)
    Setup {
        #[command(subcommand)]
        subcommand: Option<SetupCommands>,
    },
    /// List paired devices via the daemon API
    Devices,
    /// Show space and encryption status (direct mode, no daemon required)
    SpaceStatus,
    /// Search clipboard history (query or inspect search availability)
    Search {
        #[command(subcommand)]
        subcommand: commands::search::SearchCommands,
    },
    /// Run the daemon process inline (used internally by `start`)
    #[command(hide = true)]
    Daemon {
        /// Launched by a GUI parent that keeps stdin open for lifecycle detection
        #[arg(long)]
        gui_managed: bool,
    },
}

#[derive(Subcommand)]
enum SetupCommands {
    /// Start pairing mode and wait for another device to connect
    Pair,
    /// Connect to a device that is in pairing mode
    Connect,
    /// Inspect daemon-owned setup state
    Status,
    /// Reset daemon-owned setup state for repeatable local reruns
    Reset,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.dev {
        std::env::set_var("UNICLIPBOARD_ENV", "development");
    }

    // Handle `daemon` subcommand before creating the tokio runtime —
    // the daemon entrypoint creates its own runtime internally.
    if let Commands::Daemon { gui_managed } = cli.command {
        return uc_daemon::entrypoint::run(gui_managed);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let exit_code = rt.block_on(async {
        match cli.command {
            Commands::Start { foreground } => {
                commands::start::run(foreground, cli.json, cli.verbose).await
            }
            Commands::Stop => commands::stop::run(cli.json, cli.verbose).await,
            Commands::Status => commands::status::run(cli.json, cli.verbose).await,
            Commands::Setup { subcommand } => match subcommand {
                None => commands::setup::run_interactive(cli.json, cli.verbose).await,
                Some(SetupCommands::Pair) => commands::setup::run_pair(cli.json, cli.verbose).await,
                Some(SetupCommands::Connect) => {
                    commands::setup::run_connect(cli.json, cli.verbose).await
                }
                Some(SetupCommands::Status) => {
                    commands::setup::run_status(cli.json, cli.verbose).await
                }
                Some(SetupCommands::Reset) => {
                    commands::setup::run_reset(cli.json, cli.verbose).await
                }
            },
            Commands::Devices => commands::devices::run(cli.json, cli.verbose).await,
            Commands::SpaceStatus => commands::space_status::run(cli.json, cli.verbose).await,
            Commands::Search { subcommand } => {
                commands::search::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Daemon { .. } => unreachable!("handled above"),
        }
    });

    std::process::exit(exit_code);
}
