//! `uniclip mobile-sync` —— 移动端同步管理命令组(Phase 4)。
//!
//! 子命令一览(详见 `.context/mobile-sync/SPEC.md` §7):
//!
//! | 写命令(daemon 必须 stop) | 读命令(daemon 跑时也允许) |
//! |---|---|
//! | `enable` / `disable` | `settings show` |
//! | `lan enable` / `lan disable` | `lan list-interfaces` |
//! | `devices revoke` | `devices list` |
//! | `shortcut add` | — |
//!
//! 全部 in-process 调 [`MobileSyncFacade`],不走 daemon HTTP API
//! (项目惯例,详见 `uc-cli/AGENTS.md`)。写命令在执行前调
//! [`refuse_if_daemon_running`] 拒绝同 profile 多进程。
//!
//! [`MobileSyncFacade`]: uc_application::facade::MobileSyncFacade
//! [`refuse_if_daemon_running`]: crate::commands::app_session::refuse_if_daemon_running

use clap::Subcommand;

pub mod devices;
pub mod disable;
pub mod enable;
pub mod lan;
pub mod settings;
mod shared;
pub mod shortcut;

#[derive(Subcommand)]
pub enum MobileSyncCommands {
    /// Enable the mobile-sync feature toggle (master switch).
    Enable,
    /// Disable the mobile-sync feature toggle (master switch).
    Disable,
    /// Settings inspection.
    Settings {
        #[command(subcommand)]
        subcommand: settings::SettingsCommands,
    },
    /// LAN listener configuration.
    Lan {
        #[command(subcommand)]
        subcommand: lan::LanCommands,
    },
    /// Paired iPhone management.
    Devices {
        #[command(subcommand)]
        subcommand: devices::DevicesCommands,
    },
    /// iOS Shortcut client onboarding.
    Shortcut {
        #[command(subcommand)]
        subcommand: shortcut::ShortcutCommands,
    },
}

pub async fn run(command: MobileSyncCommands, json: bool, verbose: bool) -> i32 {
    match command {
        MobileSyncCommands::Enable => enable::run(json, verbose).await,
        MobileSyncCommands::Disable => disable::run(json, verbose).await,
        MobileSyncCommands::Settings { subcommand } => {
            settings::run(subcommand, json, verbose).await
        }
        MobileSyncCommands::Lan { subcommand } => lan::run(subcommand, json, verbose).await,
        MobileSyncCommands::Devices { subcommand } => devices::run(subcommand, json, verbose).await,
        MobileSyncCommands::Shortcut { subcommand } => {
            shortcut::run(subcommand, json, verbose).await
        }
    }
}
