// stdout/stderr ARE this crate's user-facing output channel (data/JSON to
// stdout, human-readable lines via src/ui.rs per local AGENTS.md), so the
// workspace-wide print lints do not apply here.
#![allow(clippy::print_stdout, clippy::print_stderr)]

mod commands;
mod exit_codes;
mod local_daemon;
mod output;
mod setup_check;
mod ui;

use clap::{CommandFactory, Parser, Subcommand};

/// Initialise AppKit enough for headless macOS CLI invocations.
///
/// Only needed when the `dev-tools` feature is enabled — in that mode the
/// CLI may build an in-process `CliAppSession` which calls
/// `clipboard-rs`'s `+[NSPasteboard generalPasteboard]`. Without dev-tools
/// the CLI is a pure daemon client and never touches AppKit.
#[cfg(all(target_os = "macos", feature = "dev-tools"))]
fn init_macos_appkit() {
    extern "C" {
        fn NSApplicationLoad() -> bool;
    }
    unsafe {
        let _ = NSApplicationLoad();
    }
}
#[cfg(not(all(target_os = "macos", feature = "dev-tools")))]
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
        /// Run as a headless server node (VPS / container): no system
        /// clipboard and no clipboard watcher. The node still syncs over
        /// iroh as a normal Space member and serves the mobile-sync gateway.
        /// Join the Space first (`uniclip join`) before starting.
        #[arg(long)]
        server: bool,
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
    /// Join a space with an invitation code and passphrase.
    ///
    /// Default (re-pair / first-time join) → redeems the invitation and
    /// joins the sponsor's space (joiner side of pairing). Safe to run when
    /// already in the *same* space: stale member/trust rows are replaced in
    /// the new handshake (issue #1023), so this is how you re-pair after a
    /// one-sided unpair.
    ///
    /// `--switch` → switches to a *different* sponsor's space, re-encrypting
    /// local clipboard history under the new master key (4-phase migration:
    /// backup → handshake → swap → commit). This is destructive and prompts
    /// for confirmation; pass `--yes` to skip the prompt in non-interactive
    /// contexts. A daemon crash mid-migration auto-resumes on the next
    /// `uniclip` invocation thanks to `MigrationStatePort` persistence.
    Join {
        /// Invitation code printed by the sponsor's `invite`. Prompted
        /// interactively when omitted.
        #[arg(long)]
        code: Option<String>,
        /// Space passphrase: the sponsor's passphrase when joining, or the
        /// new sponsor's passphrase when switching. Prompted interactively
        /// when omitted.
        #[arg(long)]
        passphrase: Option<String>,
        /// Display name advertised to the sponsor as this device's name on
        /// first-time join. Defaults to the OS hostname (plus `(profile)`
        /// suffix when `--profile` is set). Persisted to settings before
        /// dialing so the B2 handshake can read it back. Ignored with
        /// `--switch`.
        #[arg(long)]
        device_name: Option<String>,
        /// Switch to a *different* sponsor's space instead of re-pairing,
        /// re-encrypting local clipboard history under the new master key.
        /// Destructive; without it `join` always takes the non-destructive
        /// re-pair path.
        #[arg(long)]
        switch: bool,
        /// Skip the confirmation prompt shown before a destructive space
        /// switch (re-encrypting local history). Required when switching
        /// non-interactively. Only meaningful together with `--switch`.
        #[arg(long)]
        yes: bool,
    },
    /// List members of this space: the local device plus paired peers.
    ///
    /// Self-contained direct mode. Prints `{name} ({state}) [local]` per
    /// member using each peer's last-known reachability. Pass `--probe` to
    /// actively ping every paired peer first so the states are fresh.
    /// Also available under the `devices` alias.
    #[command(alias = "devices")]
    Members {
        /// Actively probe paired peers for fresh online/offline state
        /// before listing (adds a network round-trip; off by default).
        #[arg(long)]
        probe: bool,
    },
    /// Dispatch one clipboard payload to paired peers.
    ///
    /// Self-contained direct mode. Two input modes, mutually exclusive:
    ///
    /// * **New entry** (default) — reads text from the positional
    ///   argument or stdin and fans it out as a fresh
    ///   `SystemClipboardSnapshot` over the V3 envelope.
    /// * **Resend** (`--resend <ENTRY-ID>`) — re-fans-out a previously
    ///   captured local entry. The CLI reconstructs the snapshot from
    ///   storage (no stdin / positional text). Fails when the entry is
    ///   remote-origin or its payload is no longer cached.
    ///
    /// Either mode accepts `--peer <DEVICE-ID>` (repeatable) to limit
    /// fan-out to specific devices. Without `--peer`, the new-entry
    /// mode dispatches to all online peers, and resend mode targets the
    /// derived `trusted_peer \ (Delivered ∪ Duplicate)` diff.
    ///
    /// EXIT CODES (resend mode):
    /// * `0` — at least one peer accepted, was a content-duplicate, or
    ///   moved into background continuation (`pending`). All-pending is
    ///   treated as success because the work has been accepted and will
    ///   resolve asynchronously via host events; the daemon writes the
    ///   delivery record on real completion.
    /// * Non-zero — every target ended up `offline` or `errored` (no
    ///   accepted, no duplicate, no pending). Use `--json` to inspect
    ///   per-bucket counts when a CI harness needs finer-grained checks.
    Send {
        /// Plaintext to send. Omit to read from stdin until EOF.
        /// Mutually exclusive with `--resend` and `--file`.
        #[arg(conflicts_with_all = ["resend", "file"])]
        text: Option<String>,
        /// Send a file instead of text. Publishes the file as a blob to
        /// the iroh-blobs store, dispatches a clipboard envelope to
        /// online peers, then keeps the process alive so peers can fetch
        /// the bytes. Press Ctrl-C to stop serving. Mutually exclusive
        /// with `--resend`.
        #[arg(
            short = 'f',
            long = "file",
            value_name = "PATH",
            conflicts_with = "resend"
        )]
        file: Option<std::path::PathBuf>,
        /// Re-fan-out an existing entry by its ID instead of sending
        /// new text. When set, stdin is not consumed.
        #[arg(long, value_name = "ENTRY-ID")]
        resend: Option<String>,
        /// Restrict fan-out to the listed device IDs. Repeat the flag
        /// for multiple peers (e.g. `--peer dev-a --peer dev-b`).
        #[arg(long = "peer", value_name = "DEVICE-ID")]
        peers: Vec<String>,
    },
    /// Watch inbound clipboard payloads from paired peers and print each
    /// delivery as it lands. Press Ctrl-C to stop.
    ///
    /// Self-contained direct mode. Decodes the V3 envelope and shows the
    /// first text representation (or a per-rep summary for image-only
    /// envelopes). Does NOT write the system clipboard — that's the
    /// daemon's job; the CLI watch is purely a diagnostic observer.
    Watch,
    /// Receive a single inbound file from a paired peer and save it to
    /// disk. Exits after the first file arrives (or on Ctrl-C).
    ///
    /// Daemon-client mode: connects to a running daemon (or spawns a
    /// transient one), waits for the first inbound clipboard entry that
    /// carries a materialized file, exports its bytes from the daemon, and
    /// writes them into the output directory. Press Ctrl-C to stop waiting.
    /// Does NOT write the system clipboard — recv is strictly a file sink.
    Recv {
        /// Output directory. Created if missing. Defaults to current
        /// working directory.
        #[arg(short = 'o', long = "out", value_name = "DIR")]
        out: Option<std::path::PathBuf>,
    },
    /// Read an already-synced clipboard entry and return immediately.
    ///
    /// Unlike `recv` (which blocks waiting for the NEXT inbound file), `get`
    /// reads what is already in the daemon's history — ideal for headless /
    /// SSH boxes with no system clipboard, and for scripts / agents.
    ///
    /// Selection (default: the newest usable entry):
    /// * `--type <image|file|text|link>` — newest entry of that kind.
    /// * `--id <ENTRY-ID>` — a specific entry (see `uniclip search`).
    /// * `--list` — list recent entries instead of materializing one.
    ///
    /// Output: text/link content prints to stdout; image/file bytes are
    /// written to `--out` (default cache dir) with the absolute path printed
    /// to stdout, or streamed to stdout with `--out -`. Status lines go to
    /// stderr, so stdout stays clean for piping.
    ///
    /// EXIT CODES: `0` materialized; `6` no entry matched the selector;
    /// `7` matched but payload unavailable (Lost / not downloaded — re-send
    /// from the source device).
    Get {
        /// Restrict selection to the newest entry of this kind.
        #[arg(long = "type", value_name = "KIND", value_enum)]
        kind: Option<commands::get::GetKind>,
        /// Select a specific entry by id (from `uniclip search`).
        #[arg(long, value_name = "ENTRY-ID", conflicts_with = "kind")]
        id: Option<String>,
        /// List recent entries instead of materializing one.
        #[arg(long, conflicts_with_all = ["kind", "id"])]
        list: bool,
        /// Number of recent entries to scan / list (default 50).
        #[arg(short = 'n', long, value_name = "N")]
        limit: Option<usize>,
        /// Output for image/file bytes: a directory, or `-` for stdout.
        /// Defaults to a per-user cache directory. Ignored for text/link
        /// (those always print to stdout).
        #[arg(short = 'o', long = "out", value_name = "DIR|-")]
        out: Option<String>,
    },
    /// Publish or fetch encrypted large payload blobs
    #[cfg(feature = "dev-tools")]
    Blob {
        #[command(subcommand)]
        subcommand: commands::blob::BlobCommands,
    },
    /// Search clipboard history. Provide a query to search, or use the
    /// `status` / `rebuild` subcommands to inspect or maintain the index.
    #[command(args_conflicts_with_subcommands = true)]
    Search {
        #[command(flatten)]
        query: commands::search::SearchQueryArgs,
        #[command(subcommand)]
        subcommand: Option<commands::search::SearchCommands>,
    },
    /// Inspect or advance the upgrade-detection cursor (manual verification
    /// for the P1 thin upgrade module). Bare `upgrade` prints status; use
    /// the `ack` subcommand to advance the cursor.
    Upgrade {
        #[command(subcommand)]
        subcommand: Option<commands::upgrade::UpgradeCommands>,
    },
    /// Manage persistent local debug logging and export diagnostic logs
    Debug {
        #[command(subcommand)]
        subcommand: commands::debug::DebugCommands,
    },
    /// Hidden clipboard-diagnostic subcommand group (replaces the standalone
    /// `clipboard-probe` binary). Development and E2E debugging only.
    #[cfg(feature = "dev-tools")]
    #[command(hide = true)]
    Probe {
        #[command(subcommand)]
        subcommand: commands::probe::ProbeCommands,
    },
    /// Hidden development tools.
    #[cfg(feature = "dev-tools")]
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

    let Some(command) = cli.command else {
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    let exit_code = rt.block_on(async {
        match command {
            Commands::Start { foreground, server } => {
                commands::start::run(foreground, server, cli.json, cli.verbose).await
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
                switch,
                yes,
            } => {
                commands::join::run(
                    commands::join::JoinArgs {
                        code,
                        passphrase,
                        device_name,
                        switch,
                        yes,
                    },
                    cli.verbose,
                )
                .await
            }
            Commands::Members { probe } => {
                commands::members::run(probe, cli.json, cli.verbose).await
            }
            Commands::Send {
                text,
                file,
                resend,
                peers,
            } => {
                commands::send::run(
                    commands::send::SendArgs {
                        text,
                        file,
                        resend,
                        peers,
                    },
                    cli.json,
                    cli.verbose,
                )
                .await
            }
            Commands::Watch => commands::watch::run(cli.json, cli.verbose).await,
            Commands::Recv { out } => commands::recv::run(out, cli.json, cli.verbose).await,
            Commands::Get {
                kind,
                id,
                list,
                limit,
                out,
            } => {
                commands::get::run(
                    commands::get::GetArgs {
                        kind,
                        id,
                        list,
                        limit,
                        out,
                    },
                    cli.json,
                    cli.verbose,
                )
                .await
            }
            #[cfg(feature = "dev-tools")]
            Commands::Blob { subcommand } => {
                commands::blob::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Search { query, subcommand } => {
                commands::search::run(query, subcommand, cli.json, cli.verbose).await
            }
            Commands::Upgrade { subcommand } => {
                commands::upgrade::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::Debug { subcommand } => {
                commands::debug::run(subcommand, cli.json, cli.verbose).await
            }
            #[cfg(feature = "dev-tools")]
            Commands::Probe { subcommand } => commands::probe::run(subcommand, cli.verbose).await,
            #[cfg(feature = "dev-tools")]
            Commands::Dev { subcommand } => {
                commands::dev::run(subcommand, cli.json, cli.verbose).await
            }
            Commands::MobileSync { subcommand } => {
                commands::mobile_sync::run(subcommand, cli.json, cli.verbose).await
            }
        }
    });

    std::process::exit(exit_code);
}

#[cfg(test)]
mod tests {
    use super::{commands, Cli, Commands};
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
    fn switch_space_command_is_removed() {
        let result = Cli::try_parse_from(["uniclip", "switch-space", "--code", "ABCD-1234"]);

        assert!(
            result.is_err(),
            "switch-space is merged into `join`; the standalone command must be gone"
        );
    }

    #[test]
    fn join_accepts_switch_and_yes_flags() {
        // `--switch` opts into the destructive migration path; `--yes` skips
        // its confirmation in non-interactive contexts.
        let cli = Cli::try_parse_from([
            "uniclip",
            "join",
            "--code",
            "ABCD-1234",
            "--passphrase",
            "pw",
            "--switch",
            "--yes",
        ])
        .expect("join must accept --switch and --yes");
        let Some(Commands::Join { switch, yes, .. }) = cli.command else {
            panic!("expected Join command");
        };
        assert!(switch, "--switch must parse into the Join command");
        assert!(yes, "--yes must parse into the Join command");
    }

    #[test]
    fn join_defaults_to_re_pair_without_switch() {
        // A bare `join` (no `--switch`) must route to the non-destructive
        // redeem / re-pair path regardless of setup state (issue #1023).
        let cli = Cli::try_parse_from([
            "uniclip",
            "join",
            "--code",
            "ABCD-1234",
            "--passphrase",
            "pw",
        ])
        .expect("bare join must parse");
        let Some(Commands::Join { switch, .. }) = cli.command else {
            panic!("expected Join command");
        };
        assert!(!switch, "join must default to re-pair, not switch");
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
    fn search_query_is_flattened_to_top_level() {
        // `search <query>` no longer requires the `query` subcommand.
        let cli = Cli::try_parse_from(["uniclip", "search", "report", "--type", "text"])
            .expect("flattened search query must parse");
        let Some(Commands::Search { subcommand, .. }) = cli.command else {
            panic!("expected Search command");
        };
        assert!(
            subcommand.is_none(),
            "a bare query must not be parsed as a subcommand"
        );
    }

    #[test]
    fn search_status_still_parses_as_subcommand() {
        let cli =
            Cli::try_parse_from(["uniclip", "search", "status"]).expect("search status must parse");
        let Some(Commands::Search { subcommand, .. }) = cli.command else {
            panic!("expected Search command");
        };
        assert!(matches!(
            subcommand,
            Some(commands::search::SearchCommands::Status)
        ));
    }

    #[test]
    fn bare_upgrade_defaults_to_no_subcommand() {
        // `uniclip upgrade` (no subcommand) is valid and means "show status".
        let cli = Cli::try_parse_from(["uniclip", "upgrade"]).expect("bare upgrade must parse");
        let Some(Commands::Upgrade { subcommand }) = cli.command else {
            panic!("expected Upgrade command");
        };
        assert!(subcommand.is_none());
    }

    #[test]
    fn upgrade_ack_parses_as_subcommand() {
        let cli =
            Cli::try_parse_from(["uniclip", "upgrade", "ack"]).expect("upgrade ack must parse");
        let Some(Commands::Upgrade { subcommand }) = cli.command else {
            panic!("expected Upgrade command");
        };
        assert!(matches!(
            subcommand,
            Some(commands::upgrade::UpgradeCommands::Ack)
        ));
    }

    #[test]
    fn members_probe_flag_parses_and_defaults_off() {
        let bare = Cli::try_parse_from(["uniclip", "members"]).expect("bare members must parse");
        let Some(Commands::Members { probe }) = bare.command else {
            panic!("expected Members command");
        };
        assert!(!probe, "probe must default off");

        let probed =
            Cli::try_parse_from(["uniclip", "members", "--probe"]).expect("members --probe parses");
        let Some(Commands::Members { probe }) = probed.command else {
            panic!("expected Members command");
        };
        assert!(probe);
    }

    #[test]
    fn devices_is_an_alias_for_members() {
        // The former `devices` command is now a hidden alias of `members`.
        let cli = Cli::try_parse_from(["uniclip", "devices"]).expect("devices alias must parse");
        assert!(matches!(cli.command, Some(Commands::Members { .. })));
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
    fn mobile_sync_network_set_requires_an_advertise_target() {
        // `network set` 必须强制给出一个广告目标 —— iPhone 客户端需要一个
        // 具体可达的地址写进 install URL;daemon 自己始终绑 0.0.0.0,与
        // advertise 无关。两种形态二选一(`advertise_target` ArgGroup)。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "network", "set"]);
        assert!(
            result.is_err(),
            "expected `network set` to require --ip or --url"
        );
    }

    #[test]
    fn mobile_sync_network_set_accepts_url() {
        // 反代形态:`--url` 单独给出即可满足 advertise_target 组。
        let result = Cli::try_parse_from([
            "uniclip",
            "mobile-sync",
            "network",
            "set",
            "--url",
            "https://clip.example.com",
            "--accept-network-risk",
        ]);
        assert!(result.is_ok(), "expected `network set --url` to parse");
    }

    #[test]
    fn mobile_sync_network_set_rejects_both_forms() {
        // 互斥:同时给 --ip 和 --url 必须被 ArgGroup 拒绝。
        let result = Cli::try_parse_from([
            "uniclip",
            "mobile-sync",
            "network",
            "set",
            "--ip",
            "192.168.1.5",
            "--url",
            "https://clip.example.com",
            "--accept-network-risk",
        ]);
        assert!(
            result.is_err(),
            "expected `network set` to reject both --ip and --url at once"
        );
    }

    #[test]
    fn mobile_sync_network_off_and_interfaces_parse() {
        // `network off` / `network interfaces` 是无参子命令,锁住解析契约。
        let off = Cli::try_parse_from(["uniclip", "mobile-sync", "network", "off"]);
        assert!(off.is_ok(), "expected `network off` to parse");
        let ifaces = Cli::try_parse_from(["uniclip", "mobile-sync", "network", "interfaces"]);
        assert!(ifaces.is_ok(), "expected `network interfaces` to parse");
    }

    #[test]
    fn mobile_sync_add_requires_label() {
        // 顶层 `add`(原 `devices add`)—— `--label` 必填,否则 register
        // flow 拿不到设备名。
        let missing = Cli::try_parse_from(["uniclip", "mobile-sync", "add"]);
        assert!(missing.is_err(), "expected `add` to require --label");
        let ok = Cli::try_parse_from(["uniclip", "mobile-sync", "add", "--label", "My iPhone"]);
        assert!(ok.is_ok(), "expected `add --label` to parse");
    }

    #[test]
    fn mobile_sync_revoke_id_optional() {
        // 顶层 `revoke` device_id 可选(无 id 走交互式选)。clap 解析层应
        // 允许两种形态。
        let r1 = Cli::try_parse_from(["uniclip", "mobile-sync", "revoke"]);
        assert!(r1.is_ok(), "expected `revoke` (no id) to parse");
        let r2 = Cli::try_parse_from(["uniclip", "mobile-sync", "revoke", "did_abc"]);
        assert!(r2.is_ok(), "expected `revoke <id>` to parse");
    }

    #[test]
    fn mobile_sync_legacy_command_groups_are_removed() {
        // 干净删除(无 deprecation 周期):旧分组 `lan` / `devices` /
        // `settings` 已不再解析 —— 改用 `network` / 顶层 `add`·`revoke` /
        // `status`。
        for args in [
            vec!["uniclip", "mobile-sync", "lan", "list-interfaces"],
            vec!["uniclip", "mobile-sync", "lan", "enable"],
            vec!["uniclip", "mobile-sync", "lan", "disable"],
            vec!["uniclip", "mobile-sync", "devices", "list"],
            vec!["uniclip", "mobile-sync", "devices", "add", "--label", "X"],
            vec!["uniclip", "mobile-sync", "devices", "revoke", "did_abc"],
            vec!["uniclip", "mobile-sync", "settings", "show"],
        ] {
            let pretty = args.join(" ");
            assert!(
                Cli::try_parse_from(args).is_err(),
                "expected removed legacy command `{pretty}` to no longer parse"
            );
        }
    }

    #[test]
    fn mobile_sync_shortcut_subcommand_is_removed() {
        // 拓扑重组:`shortcut add` 已搬到 `add` / `devices add`,老路径
        // 直接删除(无 deprecation 周期)。
        let result =
            Cli::try_parse_from(["uniclip", "mobile-sync", "shortcut", "add", "--label", "X"]);
        assert!(
            result.is_err(),
            "expected `shortcut` subcommand to be removed"
        );
    }

    #[test]
    fn mobile_sync_enable_subcommand_is_removed() {
        // 拓扑重组:顶层 `enable` 与 `setup` / `network set` 重叠, 已删除。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "enable"]);
        assert!(
            result.is_err(),
            "expected `enable` subcommand to be removed"
        );
    }

    #[test]
    fn mobile_sync_status_parses() {
        // Step 4/5: 新增 `status` 综合视图(读命令)。
        let r = Cli::try_parse_from(["uniclip", "mobile-sync", "status"]);
        assert!(r.is_ok(), "expected `status` to parse");
    }

    #[cfg(feature = "dev-tools")]
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

    #[cfg(feature = "dev-tools")]
    #[test]
    fn mobile_sync_debug_put_text_requires_text() {
        // put-text 必须带 TEXT 位置参数,否则 facade 拿不到内容。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "put-text"]);
        assert!(result.is_err(), "expected `put-text` to require <TEXT>");
    }

    #[cfg(feature = "dev-tools")]
    #[test]
    fn mobile_sync_debug_put_file_requires_path() {
        // put-file 必须带 PATH;mime 是可选的。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "put-file"]);
        assert!(result.is_err(), "expected `put-file` to require <PATH>");
    }

    #[cfg(feature = "dev-tools")]
    #[test]
    fn mobile_sync_debug_get_file_requires_data_name() {
        // get-file 必须带 DATANAME 位置参数。
        let result = Cli::try_parse_from(["uniclip", "mobile-sync", "debug", "get-file"]);
        assert!(result.is_err(), "expected `get-file` to require <DATANAME>");
    }

    #[test]
    fn mobile_sync_setup_parses_with_no_args() {
        // `setup` 不强制任何 flag —— 默认全交互式。runtime 才会按
        // `--non-interactive` / `--json` 决定是否要求 --label / --ip /
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
            "--ip",
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

    #[cfg(feature = "dev-tools")]
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

    #[cfg(feature = "dev-tools")]
    #[test]
    fn dev_clipboard_seed_and_dump_commands_parse() {
        // seed/dump 是调试 / E2E 入口,已从顶层搬进隐藏的 `dev` 组。
        // 锁住新路径的解析契约 —— e2e 脚本依赖 `dev seed-clipboard` /
        // `dev dump-clipboard`。
        for args in [
            vec!["uniclip", "dev", "seed-clipboard", "--text", "hello"],
            vec!["uniclip", "dev", "dump-clipboard"],
            vec!["uniclip", "dev", "dump-clipboard", "--limit", "5"],
        ] {
            let result = Cli::try_parse_from(args.clone());
            assert!(result.is_ok(), "expected `{args:?}` to parse");
        }
    }

    #[cfg(feature = "dev-tools")]
    #[test]
    fn top_level_clipboard_seed_and_dump_are_removed() {
        // 迁移到 `dev` 组后,顶层路径必须消失,避免两套入口并存,
        // 也确保它们不再出现在公开 help 契约里。
        for args in [
            vec!["uniclip", "seed-clipboard", "--text", "hello"],
            vec!["uniclip", "dump-clipboard"],
        ] {
            let result = Cli::try_parse_from(args.clone());
            assert!(
                result.is_err(),
                "expected top-level `{args:?}` to be rejected after move under `dev`"
            );
        }
    }

    #[test]
    fn send_accepts_positional_text() {
        // 历史契约:`uniclip send hello` 必须继续工作。
        let r = Cli::try_parse_from(["uniclip", "send", "hello"]);
        assert!(r.is_ok(), "expected `send hello` to parse");
    }

    #[test]
    fn send_accepts_no_args_for_stdin_mode() {
        // `echo … | uniclip send` 链路 —— 不带 text 也不带 --resend 必须能解析。
        let r = Cli::try_parse_from(["uniclip", "send"]);
        assert!(
            r.is_ok(),
            "expected `send` with no args to parse (stdin mode)"
        );
    }

    #[test]
    fn send_resend_alone_parses() {
        let r = Cli::try_parse_from(["uniclip", "send", "--resend", "ent-123"]);
        assert!(r.is_ok(), "expected `send --resend <id>` to parse");
    }

    #[test]
    fn send_resend_with_text_is_mutually_exclusive() {
        // 互斥规则:`--resend` 不能与 positional text 同时出现。
        let r = Cli::try_parse_from(["uniclip", "send", "hello", "--resend", "ent-123"]);
        assert!(
            r.is_err(),
            "expected `send <text> --resend <id>` to fail at clap layer"
        );
    }

    #[test]
    fn send_accepts_multiple_peers() {
        // `--peer` 可重复出现;两种 mode 都允许。
        let r1 = Cli::try_parse_from([
            "uniclip", "send", "hello", "--peer", "dev-a", "--peer", "dev-b",
        ]);
        assert!(
            r1.is_ok(),
            "expected new-entry mode with multiple --peer to parse"
        );
        let r2 = Cli::try_parse_from(["uniclip", "send", "--resend", "ent-1", "--peer", "dev-a"]);
        assert!(r2.is_ok(), "expected resend mode with --peer to parse");
    }

    #[test]
    fn get_parses_with_no_args() {
        // `uniclip get` 默认取最新一条 —— 不带任何 selector 必须能解析。
        let r = Cli::try_parse_from(["uniclip", "get"]);
        assert!(r.is_ok(), "expected bare `get` to parse");
    }

    #[test]
    fn get_accepts_type_selector() {
        for kind in ["image", "file", "text", "link"] {
            let r = Cli::try_parse_from(["uniclip", "get", "--type", kind]);
            assert!(r.is_ok(), "expected `get --type {kind}` to parse");
        }
        // 非法 kind 必须被 value_enum 拒绝。
        let bad = Cli::try_parse_from(["uniclip", "get", "--type", "video"]);
        assert!(bad.is_err(), "expected `get --type video` to be rejected");
    }

    #[test]
    fn get_type_and_id_are_mutually_exclusive() {
        // 选最新某类型 与 选指定 id 互斥 —— 两者语义冲突。
        let r = Cli::try_parse_from(["uniclip", "get", "--type", "image", "--id", "ent-1"]);
        assert!(
            r.is_err(),
            "expected `get --type … --id …` to be rejected by clap"
        );
    }

    #[test]
    fn get_list_conflicts_with_selectors() {
        // `--list` 只列出, 不能同时带 selector。
        assert!(Cli::try_parse_from(["uniclip", "get", "--list", "--type", "image"]).is_err());
        assert!(Cli::try_parse_from(["uniclip", "get", "--list", "--id", "ent-1"]).is_err());
        assert!(
            Cli::try_parse_from(["uniclip", "get", "--list"]).is_ok(),
            "expected bare `get --list` to parse"
        );
    }

    #[test]
    fn get_accepts_out_dash_for_stdout() {
        // `--out -` 是把二进制导到 stdout 的契约。
        let r = Cli::try_parse_from(["uniclip", "get", "--type", "image", "--out", "-"]);
        assert!(r.is_ok(), "expected `get --type image --out -` to parse");
    }

    #[test]
    fn start_accepts_server_flag() {
        // `uniclip start --server` 是无头节点的启动契约 —— 部署脚本依赖它。
        let cli = Cli::try_parse_from(["uniclip", "start", "--server"])
            .expect("expected `start --server` to parse");
        match cli.command {
            Some(super::Commands::Start { foreground, server }) => {
                assert!(server, "--server must set the server flag");
                assert!(!foreground, "foreground must default to false");
            }
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn start_defaults_to_non_server() {
        // 不带 --server 时默认普通 daemon（保留真实系统剪贴板行为）。
        let cli = Cli::try_parse_from(["uniclip", "start"]).expect("expected `start` to parse");
        match cli.command {
            Some(super::Commands::Start { server, .. }) => {
                assert!(!server, "plain `start` must not enable server mode");
            }
            _ => panic!("expected Start command"),
        }
    }

    #[test]
    fn start_server_with_foreground_parses() {
        // `--server` 与 `--foreground` 可叠加（调试时前台跑 server）。
        let cli = Cli::try_parse_from(["uniclip", "start", "--server", "--foreground"])
            .expect("expected `start --server --foreground` to parse");
        match cli.command {
            Some(super::Commands::Start { foreground, server }) => {
                assert!(server);
                assert!(foreground);
            }
            _ => panic!("expected Start command"),
        }
    }
}
