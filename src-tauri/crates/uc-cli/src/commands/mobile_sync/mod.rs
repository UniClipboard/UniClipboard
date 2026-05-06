//! `uniclip mobile-sync` —— 移动端同步管理命令组。
//!
//! Step 4/5 重组后的命令拓扑(详见 `.context/mobile-sync-cli-redesign/`):
//!
//! | 写命令(daemon 必须 stop) | 读命令(daemon 跑时也允许) |
//! |---|---|
//! | `setup`(一键向导)| `status`(综合视图) |
//! | `disable`(关总开关 + 关 LAN)| `settings show`(advanced) |
//! | `lan enable / disable`(advanced)| `lan list-interfaces`(advanced) |
//! | `devices add / revoke` | `devices list` |
//!
//! 老命令 `enable` / `shortcut add` 已删除:
//! - `enable` 与 `lan enable` 重叠 + `setup` 一键已含, 无独立价值
//! - `shortcut add` 改名为更直观的 `devices add`(同时支持自定义凭据 flags)
//!
//! 全部 in-process 调 [`MobileSyncFacade`],不走 daemon HTTP API
//! (项目惯例,详见 `uc-cli/AGENTS.md`)。写命令在执行前调
//! [`refuse_if_daemon_running`] 拒绝同 profile 多进程。
//!
//! [`MobileSyncFacade`]: uc_application::facade::MobileSyncFacade
//! [`refuse_if_daemon_running`]: crate::commands::app_session::refuse_if_daemon_running

use clap::Subcommand;

pub mod debug;
pub mod devices;
pub mod disable;
pub mod lan;
pub mod settings;
pub mod setup;
mod shared;
pub mod status;

#[derive(Subcommand)]
pub enum MobileSyncCommands {
    /// One-shot setup wizard: enables the feature, configures the LAN
    /// listener, registers an iPhone, and prints the install QR + a
    /// one-time password — all in a single command.
    Setup(setup::SetupArgs),
    /// Combined status view: feature + LAN settings + paired devices +
    /// install methods. Daemon-running tolerant.
    Status,
    /// Disable mobile-sync entirely: master switch off + LAN listener off.
    /// Paired devices stay registered (use `devices revoke` to drop them).
    Disable,
    /// Settings inspection (advanced — prefer `status`).
    Settings {
        #[command(subcommand)]
        subcommand: settings::SettingsCommands,
    },
    /// LAN listener configuration (advanced — `setup` already handles the
    /// common case).
    Lan {
        #[command(subcommand)]
        subcommand: lan::LanCommands,
    },
    /// Paired iPhone management.
    Devices {
        #[command(subcommand)]
        subcommand: devices::DevicesCommands,
    },
    /// Debug helpers that simulate the SyncClipboard protocol locally
    /// (no iPhone required). Bypasses HTTP and calls `MobileSyncFacade`
    /// directly. All subcommands require the daemon to be stopped.
    ///
    /// `#[command(hide=true)]` keeps these out of the public `--help`
    /// surface — they are dev / E2E only(`scripts/test_mobile_sync_debug_e2e.sh`),
    /// not user-facing. Still callable explicitly.
    #[command(hide = true)]
    Debug {
        #[command(subcommand)]
        subcommand: debug::DebugCommands,
    },
}

pub async fn run(command: MobileSyncCommands, json: bool, verbose: bool) -> i32 {
    match command {
        MobileSyncCommands::Setup(args) => setup::run(args, json, verbose).await,
        MobileSyncCommands::Status => status::run(json, verbose).await,
        MobileSyncCommands::Disable => disable::run(json, verbose).await,
        MobileSyncCommands::Settings { subcommand } => {
            settings::run(subcommand, json, verbose).await
        }
        MobileSyncCommands::Lan { subcommand } => lan::run(subcommand, json, verbose).await,
        MobileSyncCommands::Devices { subcommand } => devices::run(subcommand, json, verbose).await,
        MobileSyncCommands::Debug { subcommand } => debug::run(subcommand, json, verbose).await,
    }
}
