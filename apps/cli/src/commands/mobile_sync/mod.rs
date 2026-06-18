//! `uniclip mobile` —— 移动端同步管理命令组(旧名 `mobile-sync` 仍作为隐藏
//! 弃用别名保留,调用时打印迁移提示,见 `main.rs` 的 dispatcher)。
//!
//! 任务导向的命令拓扑 —— 顶层只暴露用户高频动作,网络细节收进 `network`:
//!
//! | 动作 | 命令 |
//! |---|---|
//! | 首次一键向导 | `setup` |
//! | 新增一台 iPhone | `add` |
//! | 移除一台 iPhone | `revoke [<device-id>]` |
//! | 查看全部状态 | `status` |
//! | 完全停用 | `disable`(关总开关 + 关 LAN) |
//! | 高级网络配置 | `network interfaces / set / off`(advanced) |
//!
//! P5-2b (ADR-008): all non-debug commands route through daemon HTTP
//! endpoints via [`DaemonMobileSyncClient`]. The hidden `debug` subcommand
//! (P5-3 scope) still uses in-process [`MobileSyncFacade`] directly.
//!
//! [`DaemonMobileSyncClient`]: uc_daemon_client::http::DaemonMobileSyncClient
//! [`MobileSyncFacade`]: uc_application::facade::MobileSyncFacade

use clap::Subcommand;

#[cfg(feature = "dev-tools")]
pub mod debug;
pub mod devices;
pub mod disable;
pub mod network;
pub mod setup;
mod shared;
pub mod status;

#[derive(Subcommand)]
pub enum MobileSyncCommands {
    /// One-shot setup wizard: enables the feature, configures the LAN
    /// listener, registers an iPhone, and prints the install QR + a
    /// one-time password — all in a single command.
    Setup(setup::SetupArgs),
    /// Pair a new iPhone: mint credentials and print the install QR. Use this
    /// to add another phone after the initial `setup`.
    Add(devices::AddArgs),
    /// Unpair an iPhone. Without `<device-id>`, interactively pick from the
    /// paired list (JSON mode requires the id explicitly).
    Revoke {
        /// Device id printed by `status` (e.g. `did_<32hex>`).
        device_id: Option<String>,
    },
    /// Combined status view: feature + LAN settings + paired devices +
    /// install methods. Daemon-running tolerant.
    Status,
    /// Disable mobile-sync entirely: master switch off + LAN listener off.
    /// Paired devices stay registered (use `revoke` to drop them). To stop
    /// only the LAN listener, use `network off`.
    Disable,
    /// Advanced LAN / reverse-proxy listener configuration. `setup` already
    /// handles the common case.
    Network {
        #[command(subcommand)]
        subcommand: network::NetworkCommands,
    },
    /// Debug helpers that simulate the SyncClipboard protocol locally
    /// (no iPhone required). Bypasses HTTP and calls `MobileSyncFacade`
    /// directly. All subcommands require the daemon to be stopped.
    ///
    /// `#[command(hide=true)]` keeps these out of the public `--help`
    /// surface — they are dev / E2E only(`scripts/test_mobile_sync_debug_e2e.sh`),
    /// not user-facing. Still callable explicitly.
    #[cfg(feature = "dev-tools")]
    #[command(hide = true)]
    Debug {
        #[command(subcommand)]
        subcommand: debug::DebugCommands,
    },
}

pub async fn run(command: MobileSyncCommands, json: bool, verbose: bool) -> i32 {
    match command {
        MobileSyncCommands::Setup(args) => setup::run(args, json, verbose).await,
        MobileSyncCommands::Add(args) => devices::add(args, json, verbose).await,
        MobileSyncCommands::Revoke { device_id } => devices::revoke(device_id, json, verbose).await,
        MobileSyncCommands::Status => status::run(json, verbose).await,
        MobileSyncCommands::Disable => disable::run(json, verbose).await,
        MobileSyncCommands::Network { subcommand } => network::run(subcommand, json, verbose).await,
        #[cfg(feature = "dev-tools")]
        MobileSyncCommands::Debug { subcommand } => debug::run(subcommand, json, verbose).await,
    }
}
