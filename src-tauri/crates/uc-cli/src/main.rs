mod commands;
mod exit_codes;
mod local_daemon;
mod output;
mod ui;

use clap::{CommandFactory, Parser, Subcommand};

/// Initialise AppKit enough for headless macOS CLI invocations.
///
/// `clipboard-rs` eagerly calls `+[NSPasteboard generalPasteboard]` during
/// `wire_dependencies`, which returns NULL and panics when the process
/// has not loaded AppKit (typical for a CLI launched from a shell that
/// does not carry a proper Cocoa context). `NSApplicationLoad` is the
/// documented way to bootstrap AppKit in non-`.app` processes.
#[cfg(target_os = "macos")]
fn init_macos_appkit() {
    extern "C" {
        fn NSApplicationLoad() -> bool;
    }
    unsafe {
        let _ = NSApplicationLoad();
    }
}
#[cfg(not(target_os = "macos"))]
fn init_macos_appkit() {}

#[derive(Parser)]
#[command(
    name = "uniclip",
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

    /// Override the active profile (equivalent to `UC_PROFILE`). Isolates
    /// data dir, keychain, and iroh identity — needed to run two CLI
    /// instances on the same machine for end-to-end pairing testing.
    #[arg(long, global = true, value_name = "NAME")]
    profile: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
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
    /// Show application status
    Status,
    /// Create a new encrypted space for this profile.
    ///
    /// Use this on the first device before inviting other devices.
    Init {
        /// Space passphrase. If omitted, prompts interactively with
        /// confirmation. Pass this flag only in non-interactive contexts
        /// such as the single-machine e2e test script.
        #[arg(long)]
        passphrase: Option<String>,
        /// Display name advertised to paired peers. Defaults to the OS
        /// hostname (plus `(profile)` suffix when `--profile` is set).
        #[arg(long)]
        device_name: Option<String>,
    },
    /// Issue a pairing invitation and wait for a joiner (sponsor side).
    ///
    /// Silently resumes the local session from the KEK cached in
    /// keychain (or `--dev`'s file secure storage) by a prior `init` /
    /// `unlock` — no passphrase re-entry needed. Fails if the profile
    /// has not been initialized yet.
    Invite,
    /// Redeem an invitation and join an existing space (joiner side).
    Join {
        /// Invitation code printed by the sponsor's `invite`. Prompted
        /// interactively when omitted.
        #[arg(long)]
        code: Option<String>,
        /// Space passphrase the sponsor chose during `init`. Prompted
        /// interactively when omitted.
        #[arg(long)]
        passphrase: Option<String>,
        /// Display name advertised to the sponsor as this device's name.
        /// Defaults to the OS hostname (plus `(profile)` suffix when
        /// `--profile` is set). Persisted to settings before dialing so
        /// the B2 handshake can read it back.
        #[arg(long)]
        device_name: Option<String>,
    },
    /// Debug / E2E only: insert one text clipboard entry encrypted with
    /// the current session master key. Used by switch-space data-integrity
    /// tests as a seeding helper. Not part of the user-facing surface.
    SeedClipboard {
        /// Plaintext to seed.
        #[arg(long)]
        text: String,
    },
    /// Debug / E2E only: print the latest decrypted clipboard entries
    /// (preview field is plaintext after decryption). Pair with
    /// `seed-clipboard` to verify switch-space preserves data round-trip.
    DumpClipboard {
        /// Maximum number of entries to print (default 10).
        #[arg(long, default_value_t = 10)]
        limit: usize,
    },
    /// Switch to another sponsor's space, re-encrypting local clipboard
    /// history under the new master key (4-phase migration).
    ///
    /// Pre-condition: this device has already completed `init` or `join`.
    /// Runs the full re-encryption pipeline: backup → handshake → swap →
    /// commit. A daemon crash mid-run resumes automatically on the next
    /// `uniclip` invocation thanks to `MigrationStatePort` persistence.
    SwitchSpace {
        /// Invitation code printed by the new sponsor's `invite`. Prompted
        /// interactively when omitted.
        #[arg(long)]
        code: Option<String>,
        /// Passphrase the new sponsor chose during `init`. Prompted
        /// interactively when omitted.
        #[arg(long)]
        new_passphrase: Option<String>,
    },
    /// List paired devices
    Devices,
    /// List members of this space with presence (online / offline / unknown).
    ///
    /// Self-contained direct mode (Slice 2 Phase 1): runs a one-off probe of
    /// all paired peers so states are fresh on every call. No daemon
    /// required. Prints `{name} ({state}) [local]` per device.
    Members,
    /// Dispatch one clipboard payload to every online paired peer.
    ///
    /// Self-contained direct mode. Reads text from the positional
    /// argument, or — when omitted — from stdin until EOF. Wraps the
    /// text into a single-representation `SystemClipboardSnapshot`,
    /// encodes it as a V3 envelope, and fans it out via the iroh
    /// clipboard ALPN — same wire format the daemon uses.
    Send {
        /// Plaintext to send. Omit to read from stdin until EOF.
        text: Option<String>,
    },
    /// Watch inbound clipboard payloads from paired peers and print each
    /// delivery as it lands. Press Ctrl-C to stop.
    ///
    /// Self-contained direct mode. Decodes the V3 envelope and shows the
    /// first text representation (or a per-rep summary for image-only
    /// envelopes). Does NOT write the system clipboard — that's the
    /// daemon's job; the CLI watch is purely a diagnostic observer.
    Watch,
    /// Publish or fetch encrypted large payload blobs
    Blob {
        #[command(subcommand)]
        subcommand: commands::blob::BlobCommands,
    },
    /// Search clipboard history (query or inspect search availability)
    Search {
        #[command(subcommand)]
        subcommand: commands::search::SearchCommands,
    },
    /// Inspect or advance the upgrade-detection cursor (manual verification
    /// for the P1 thin upgrade module).
    Upgrade {
        #[command(subcommand)]
        subcommand: commands::upgrade::UpgradeCommands,
    },
    /// Hidden clipboard-diagnostic subcommand group (replaces the standalone
    /// `clipboard-probe` binary). Development and E2E debugging only.
    #[command(hide = true)]
    Probe {
        #[command(subcommand)]
        subcommand: commands::probe::ProbeCommands,
    },
    /// Hidden development tools.
    #[command(hide = true)]
    Dev {
        #[command(subcommand)]
        subcommand: commands::dev::DevCommands,
    },
    /// Manage mobile-sync (iPhone over LAN, SyncClipboard-compatible).
    #[command(name = "mobile-sync")]
    MobileSync {
        #[command(subcommand)]
        subcommand: commands::mobile_sync::MobileSyncCommands,
    },
    /// 内联运行 daemon 进程，供 `start` 内部使用
    #[command(hide = true)]
    Daemon,
}

fn main() -> anyhow::Result<()> {
    init_macos_appkit();

    // rustls 0.23+ requires a process-wide `CryptoProvider` before any
    // `ClientConfig` is built. iroh-quinn installs its own on bind, and
    // if reqwest sees that provider first it hits
    // `CertificateError::BadEncoding` on any plain HTTPS handshake
    // (symptom: "bad certificate format" from `rendezvous.uniclipboard.app`).
    // Setting ring as the explicit default up front keeps both stacks on
    // the same provider. The call is idempotent and safe if something
    // already installed a provider.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = Cli::parse();

    if cli.dev {
        std::env::set_var("UNICLIPBOARD_ENV", "development");
    }

    // `--profile <name>` must land as `UC_PROFILE` BEFORE any bootstrap
    // code runs, because `wire_dependencies` + `apply_profile_suffix`
    // branch on that env var at path-resolution time.
    if let Some(profile) = &cli.profile {
        std::env::set_var("UC_PROFILE", profile);
    }

    // Handle `daemon` subcommand before creating the tokio runtime —
    // the daemon entrypoint creates its own runtime internally.
    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    if let Commands::Daemon = command {
        // `uniclip` 二进制既能当 CLI 又能当 standalone daemon,光看
        // current_exe 区分不出来。在调 bootstrap 之前先打这个标记,让
        // `uc_observability::ScopeContext::resolve` 把本进程的 device.role
        // 标成 `daemon`,Sentry 上能清楚区分 daemon 事件与 CLI 事件。
        std::env::set_var("UC_HOST_ROLE", "daemon");

        // CLI `start` detached-spawns this same binary with the `daemon`
        // subcommand. Standalone is the only mode this binary ever runs in
        // since the GUI has been switched to in-process daemon startup.
        return uc_desktop::daemon::run(uc_desktop::daemon::run_mode::DaemonRunMode::Standalone);
    }

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let exit_code = rt.block_on(async {
        match command {
            Commands::Start { foreground } => {
                commands::start::run(foreground, cli.json, cli.verbose).await
            }
            Commands::Stop => commands::stop::run(cli.json, cli.verbose).await,
            Commands::Status => commands::status::run(cli.json, cli.verbose).await,
            Commands::Init {
                passphrase,
                device_name,
            } => {
                commands::init::run(
                    commands::init::InitArgs {
                        passphrase,
                        device_name,
                    },
                    cli.verbose,
                )
                .await
            }
            Commands::Invite => commands::invite::run(cli.verbose).await,
            Commands::Join {
                code,
                passphrase,
                device_name,
            } => {
                commands::join::run(
                    commands::join::JoinArgs {
                        code,
                        passphrase,
                        device_name,
                    },
                    cli.verbose,
                )
                .await
            }
            Commands::SeedClipboard { text } => {
                commands::seed_clipboard::run(
                    commands::seed_clipboard::SeedClipboardArgs { text },
                    cli.verbose,
                )
                .await
            }
            Commands::DumpClipboard { limit } => {
                commands::dump_clipboard::run(
                    commands::dump_clipboard::DumpClipboardArgs { limit },
                    cli.json,
                    cli.verbose,
                )
                .await
            }
            Commands::SwitchSpace {
                code,
                new_passphrase,
            } => {
                commands::switch_space::run(
                    commands::switch_space::SwitchSpaceArgs {
                        code,
                        new_passphrase,
                    },
                    cli.verbose,
                )
                .await
            }
            Commands::Devices => commands::devices::run(cli.json, cli.verbose).await,
            Commands::Members => commands::members::run(cli.json, cli.verbose).await,
            Commands::Send { text } => {
                commands::send::run(commands::send::SendArgs { text }, cli.json, cli.verbose).await
            }
            Commands::Watch => commands::watch::run(cli.json, cli.verbose).await,
            Commands::Blob { subcommand } => {
                commands::blob::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Search { subcommand } => {
                commands::search::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Upgrade { subcommand } => {
                commands::upgrade::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Probe { subcommand } => commands::probe::run(subcommand, cli.verbose).await,
            Commands::Dev { subcommand } => {
                commands::dev::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::MobileSync { subcommand } => {
                commands::mobile_sync::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Daemon => unreachable!("handled above"),
        }
    });

    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::{CommandFactory, Parser};

    #[test]
    fn cli_binary_name_is_uniclip() {
        let command = Cli::command();

        assert_eq!(command.get_name(), "uniclip");
    }

    #[test]
    fn no_subcommand_displays_help() {
        let result = Cli::try_parse_from(["uniclip"]);

        match result {
            Ok(cli) => {
                assert!(cli.command.is_none());
            }
            Err(error) => panic!("expected no subcommand to parse successfully, got {error}"),
        }
    }

    #[test]
    fn setup_command_is_removed() {
        let result = Cli::try_parse_from(["uniclip", "setup"]);

        assert!(
            result.is_err(),
            "legacy setup command should be removed; CLI should keep init/invite/join"
        );
    }

    #[test]
    fn search_rebuild_no_wait_is_removed() {
        let result = Cli::try_parse_from(["uniclip", "search", "rebuild", "--no-wait"]);

        assert!(
            result.is_err(),
            "standalone CLI search rebuild must be synchronous and reject --no-wait"
        );
    }

    #[test]
    fn mobile_sync_kebab_case_is_accepted() {
        // 子命令名是 kebab-case `mobile-sync` 而非默认的 `mobile_sync` /
        // `mobilesync`。锁住这个外部契约 —— 改名会让所有发布的脚本失效。
        // (Step 4 起 `enable` 已删, 用 `status` 这个稳定读命令探针。)
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "status"]);
        assert!(result.is_ok(), "expected `mobile-sync status` to parse");
    }

    #[test]
    fn mobile_sync_lan_enable_requires_advertise() {
        // `lan enable` 必须强制 `--advertise <IP>` —— iPhone 客户端需要
        // 一个具体可达的 IP 写进 install URL;daemon 自己始终绑 0.0.0.0,
        // 与 advertise 无关。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "lan", "enable"]);
        assert!(
            result.is_err(),
            "expected `lan enable` to require --advertise"
        );
    }

    #[test]
    fn mobile_sync_devices_add_requires_label() {
        // `devices add` 接管原 `shortcut add` 的契约 —— `--label` 必填,
        // 否则 register flow 拿不到设备名。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "devices", "add"]);
        assert!(result.is_err(), "expected `devices add` to require --label");
    }

    #[test]
    fn mobile_sync_shortcut_subcommand_is_removed() {
        // Step 4/5 拓扑重组:`shortcut add` 已搬到 `devices add`,老路径
        // 直接删除(项目未发布,无 deprecation 周期)。
        let result =
            Cli::try_parse_from(["uniclip", "mobile-sync", "shortcut", "add", "--label", "X"]);
        assert!(
            result.is_err(),
            "expected `shortcut` subcommand to be removed"
        );
    }

    #[test]
    fn mobile_sync_enable_subcommand_is_removed() {
        // Step 4/5 拓扑重组:`enable` 与 `setup` / `lan enable` 重叠, 已删除。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "enable"]);
        assert!(
            result.is_err(),
            "expected `enable` subcommand to be removed"
        );
    }

    #[test]
    fn mobile_sync_revoke_id_optional() {
        // Step 4/5: `devices revoke` device_id 改为可选(无 id 走交互式
        // 选)。clap 解析层应允许两种形态。
        let r1 = Cli::try_parse_from(["uniclip", "mobile-sync", "devices", "revoke"]);
        assert!(r1.is_ok(), "expected `devices revoke` (no id) to parse");
        let r2 = Cli::try_parse_from(["uniclip", "mobile-sync", "devices", "revoke", "did_abc"]);
        assert!(r2.is_ok(), "expected `devices revoke <id>` to parse");
    }

    #[test]
    fn mobile_sync_status_parses() {
        // Step 4/5: 新增 `status` 综合视图(读命令)。
        let r = Cli::try_parse_from(["uniclip", "mobile-sync", "status"]);
        assert!(r.is_ok(), "expected `status` to parse");
    }

    #[test]
    fn mobile_sync_debug_subcommands_parse() {
        // P5a.9 引入的 4 个 debug 子命令解析契约。
        for args in [
            vec!["uniclip", "mobile-sync", "debug", "put-text", "hello"],
            vec!["uniclip", "mobile-sync", "debug", "put-file", "/tmp/x.png"],
            vec!["uniclip", "mobile-sync", "debug", "get-doc"],
            vec!["uniclip", "mobile-sync", "debug", "get-file", "photo.png"],
        ] {
            let result = Cli::try_parse_from(args.clone());
            assert!(result.is_ok(), "expected `{args:?}` to parse");
        }
    }

    #[test]
    fn mobile_sync_debug_put_text_requires_text() {
        // put-text 必须带 TEXT 位置参数,否则 facade 拿不到内容。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "put-text"]);
        assert!(result.is_err(), "expected `put-text` to require <TEXT>");
    }

    #[test]
    fn mobile_sync_debug_put_file_requires_path() {
        // put-file 必须带 PATH;mime 是可选的。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "put-file"]);
        assert!(result.is_err(), "expected `put-file` to require <PATH>");
    }

    #[test]
    fn mobile_sync_debug_get_file_requires_data_name() {
        // get-file 必须带 DATANAME 位置参数。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "get-file"]);
        assert!(result.is_err(), "expected `get-file` to require <DATANAME>");
    }

    #[test]
    fn mobile_sync_setup_parses_with_no_args() {
        // `setup` 不强制任何 flag —— 默认全交互式。runtime 才会按
        // `--non-interactive` / `--json` 决定是否要求 --label / --advertise /
        // --accept-network-risk;clap 解析层不下结论。
        let r = Cli::try_parse_from(["uniclip", "mobile-sync", "setup"]);
        assert!(r.is_ok(), "expected `setup` to parse with no args");
    }

    #[test]
    fn mobile_sync_setup_accepts_full_non_interactive_flags() {
        // CI 友好的全 flag 形态。
        let r = Cli::try_parse_from([
            "uniclip",
            "mobile-sync",
            "setup",
            "--non-interactive",
            "--label",
            "iPhone",
            "--advertise",
            "192.168.1.5",
            "--port",
            "42720",
            "--username",
            "alice_001",
            "--password-stdin",
            "--accept-network-risk",
        ]);
        assert!(r.is_ok(), "expected full-flag setup to parse");
    }

    #[test]
    fn dev_pairing_manual_address_commands_parse() {
        // 隐藏开发入口用于手动选择配对地址,不进入公开 help 契约。
        for args in [
            vec!["uniclip", "dev", "pairing", "addrs"],
            vec![
                "uniclip",
                "dev",
                "pairing",
                "issue",
                "--addr",
                "100.79.191.42",
            ],
        ] {
            let result = Cli::try_parse_from(args.clone());
            assert!(result.is_ok(), "expected `{args:?}` to parse");
        }
    }
}
