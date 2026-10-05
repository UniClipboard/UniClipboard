package commands

import "github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"

// Tree is the uniclip command tree. Help texts are the user-visible contract
// carried over verbatim from the Rust CLI.
func Tree() *cli.Command {
	return &cli.Command{
		Name:   "uniclip",
		Before: applyGlobalFlags,
		About:  "UniClipboard command-line interface",
		Flags: []*cli.Flag{
			{Long: "json", Kind: cli.Bool, Global: true, Help: "Output in JSON format"},
			{Long: "verbose", Short: 'v', Kind: cli.Bool, Global: true, Help: "Enable verbose tracing output (shows debug logs on console)"},
			{Long: "dev", Kind: cli.Bool, Global: true, Hidden: true, Help: "Run in development mode (use file-based secure storage instead of system keychain)"},
			{Long: "profile", ValueName: "NAME", Kind: cli.String, Global: true, Help: "Override the active profile (equivalent to `UC_PROFILE`). Isolates data dir, keychain, and iroh identity — needed to run two CLI instances on the same machine for end-to-end pairing testing"},
		},
		Subs: []*cli.Command{
			{
				Name:  "start",
				About: "Start the daemon (background by default, use --foreground for log streaming)",
				Flags: []*cli.Flag{
					{Long: "foreground", Short: 'f', Kind: cli.Bool, Help: "Run daemon in foreground (log output to terminal)"},
					{Long: "server", Kind: cli.Bool, Help: "Run as a headless server node (VPS / container): no system clipboard and no clipboard watcher. The node still syncs over iroh as a normal Space member and serves the mobile-sync gateway. Join the Space first (`uniclip space join`) before starting"},
				},
				Run: runStart,
			},
			{
				Name:  "stop",
				About: "Stop the running daemon",
				Run:   runStop,
			},
			{
				Name:        "space",
				About:       "Manage the current encrypted space",
				SubRequired: true,
				Subs: []*cli.Command{
					{
						Name:  "status",
						About: "Show application status",
						Run:   runSpaceStatus,
					},
					{
						Name:  "init",
						About: "Create a new encrypted space for this profile",
						Flags: []*cli.Flag{
							{Long: "passphrase", ValueName: "PASSPHRASE", Kind: cli.String, Help: "Space passphrase. If omitted, prompts interactively with confirmation"},
							{Long: "device-name", ValueName: "DEVICE_NAME", Kind: cli.String, Help: "Display name advertised to paired peers"},
						},
						Run: runSpaceInit,
					},
					{
						Name:  "invite",
						About: "Issue a pairing invitation and wait for a joiner. In JSON mode, emits one event per line so callers receive the invitation before pairing completes",
						Run:   runSpaceInvite,
					},
					{
						Name:  "reset",
						About: "Rebuild this profile as a new one-device space while keeping local history",
						Flags: []*cli.Flag{
							{Long: "yes", Kind: cli.Bool, Help: "Confirm the permanent loss of all existing device relationships", Required: true},
						},
						Run: runSpaceReset,
					},
					{
						Name:  "change-passphrase",
						About: "Change the space passphrase (unlocked, single-device space only). Keeps local history",
						Flags: []*cli.Flag{
							{Long: "passphrase", ValueName: "PASSPHRASE", Kind: cli.String, Help: "New space passphrase. If omitted, prompts interactively with confirmation"},
						},
						Run: runSpaceChangePassphrase,
					},
					{
						Name:             "join",
						About:            "Join a space with an invitation code and passphrase",
						ArgsConflictSubs: true,
						Flags: []*cli.Flag{
							{Long: "code", ValueName: "CODE", Kind: cli.String, Help: "Invitation code printed by the sponsor's `space invite`"},
							{Long: "passphrase", ValueName: "PASSPHRASE", Kind: cli.String, Help: "Space passphrase"},
							{Long: "device-name", ValueName: "DEVICE_NAME", Kind: cli.String, Help: "Display name advertised to the sponsor on first-time join"},
							{Long: "switch", Kind: cli.Bool, Help: "Switch to a different space and migrate local history"},
							{Long: "yes", Kind: cli.Bool, Help: "Skip the confirmation prompt for a destructive space switch"},
							{Long: "preserve-unreadable-history", Kind: cli.Bool, Help: "Keep local history records that cannot be read during a space switch", Requires: []string{"switch"}},
							{Long: "no-wait", Kind: cli.Bool, Help: "Return after Engine accepts a pending join instead of waiting for a final result"},
						},
						Subs: []*cli.Command{
							{
								Name:  "status",
								About: "Show the current Engine-owned join status",
								Run:   runSpaceJoinStatus,
							},
							{
								Name:  "cancel",
								About: "Cancel the current pending join request",
								Run:   runSpaceJoinCancel,
							},
						},
					},
				},
			},
			{
				Name:        "member",
				About:       "Manage members of the current space",
				SubRequired: true,
				Subs: []*cli.Command{
					{
						Name:  "list",
						About: "List members of this space: the local device plus paired peers",
						Flags: []*cli.Flag{
							{Long: "probe", Kind: cli.Bool, Help: "Actively probe paired peers for fresh online/offline state before listing (adds a network round-trip; off by default)"},
						},
						Run: runMemberList,
					},
					{
						Name:      "remove",
						About:     "Record an irreversible, offline-first removal intent for one member",
						LongAbout: "Record an irreversible, offline-first removal intent for one member.\n\nImmediately stops sending new content to the peer and prints the full Engine-owned device relationship state. Pass `--json` for the raw DTO.",
						Args: []*cli.Positional{
							{Name: "PEER-ID", Help: "Peer device ID to remove", Required: true},
						},
						Run: runMemberRemove,
					},
					{
						Name:        "trust",
						About:       "Inspect or choose among current Engine-owned device groups",
						SubRequired: true,
						Subs: []*cli.Command{
							{
								Name:  "status",
								About: "Show the current device relationship state and pending group choices",
								Run:   runMemberTrustStatus,
							},
							{
								Name:  "choose",
								About: "Choose one option from the current device-group query",
								Flags: []*cli.Flag{
									{Long: "issue", ValueName: "ISSUE-ID", Kind: cli.String, Help: "Opaque issue ID from `member trust status`"},
									{Long: "choice", ValueName: "CHOICE-ID", Kind: cli.String, Help: "Opaque choice ID from the selected issue"},
									{Long: "confirm-local-removal", Kind: cli.Bool, Help: "Explicitly allow this device to be removed by the change"},
								},
								Run: runMemberTrustChoose,
							},
						},
					},
					{
						Name:        "sync",
						About:       "Inspect or update one member's sync preferences",
						SubRequired: true,
						Subs: []*cli.Command{
							{
								Name:  "show",
								About: "Show one member's current sync preferences",
								Args: []*cli.Positional{
									{Name: "DEVICE", Help: "Device ID, or an unambiguous device name in an interactive terminal", Required: true},
								},
								Run: runMemberSyncShow,
							},
							{
								Name:  "set",
								About: "Partially update one member's sync preferences",
								Flags: []*cli.Flag{
									{Long: "send", ValueName: "SEND", Kind: cli.Enum, Help: "Enable or disable sending to this member", PossibleValue: []string{"on", "off"}},
									{Long: "receive", ValueName: "RECEIVE", Kind: cli.Enum, Help: "Enable or disable receiving from this member", PossibleValue: []string{"on", "off"}},
									{Long: "send-types", ValueName: "TYPES", Kind: cli.String, Help: "Comma-separated send types, or `all` / `none`"},
									{Long: "receive-types", ValueName: "TYPES", Kind: cli.String, Help: "Comma-separated receive types, or `all` / `none`"},
								},
								Args: []*cli.Positional{
									{Name: "DEVICE", Help: "Device ID, or an unambiguous device name in an interactive terminal", Required: true},
								},
								Run: runMemberSyncSet,
							},
						},
					},
				},
			},
			{
				Name:      "send",
				About:     "Dispatch one clipboard payload to paired peers",
				LongAbout: "Dispatch one clipboard payload to paired peers.\n\nUses the local daemon. Three input modes are available:\n\n* **Automatic** (default) — an existing regular file is sent as a file; other positional input is sent as text; omitted input reads text from stdin. * **Explicit** — `--text` forces text and `--file` reads file paths from the positional argument or, when omitted, one path per stdin line. Stdin supplies paths, not file contents. * **Resend** (`--resend <ENTRY-ID>`) — re-fans-out a previously captured local entry. The CLI reconstructs the snapshot from storage (no stdin / positional text). Fails when the entry is remote-origin or its payload is no longer cached.\n\nEither mode accepts `--peer <DEVICE-ID>` (repeatable) to limit fan-out to specific devices. Without `--peer`, the new-entry mode dispatches to all online peers, and resend mode targets the derived `trusted_peer \\ (Delivered ∪ Duplicate)` diff.\n\nEXIT CODES (resend mode): * `0` — at least one peer accepted, was a content-duplicate, or moved into background continuation (`pending`). All-pending is treated as success because the work has been accepted and will resolve asynchronously via host events; the daemon writes the delivery record on real completion. * Non-zero — every target ended up `offline` or `errored` (no accepted, no duplicate, no pending). Use `--json` to inspect per-bucket counts when a CI harness needs finer-grained checks.",
				Flags: []*cli.Flag{
					{Long: "text", Kind: cli.Bool, Help: "Force the positional argument to be sent as text, even when it names an existing file", ConflictsWith: []string{"resend", "file"}},
					{Long: "file", Short: 'f', Kind: cli.Bool, Help: "Send files instead of text. With a positional argument, that value is the path. Without one, read one complete file path per stdin line; blank lines are ignored. Stdin contains paths, not file data", ConflictsWith: []string{"resend", "text"}},
					{Long: "resend", ValueName: "ENTRY-ID", Kind: cli.String, Help: "Re-fan-out an existing entry by its ID instead of sending new text. When set, stdin is not consumed"},
					{Long: "peer", ValueName: "DEVICE-ID", Kind: cli.Strings, Help: "Restrict fan-out to the listed device IDs. Repeat the flag for multiple peers (e.g. `--peer dev-a --peer dev-b`)"},
					{Long: "connect-timeout", ValueName: "SECONDS", Kind: cli.Uint, Help: "Total seconds to wait BEFORE dispatch, as one deadline shared by three stages: the local daemon becoming ready (started on demand), then the target devices connecting", LongHelp: "Total seconds to wait BEFORE dispatch, as one deadline shared by three stages: the local daemon becoming ready (started on demand), then the target devices connecting.\n\nWith `--peer`, every listed device must be connected. Without it, one connected paired device is enough (later devices are not awaited). If the deadline passes, nothing is sent: exit 5 when the daemon was not ready, exit 1 when a device was not connected. Once dispatched, a send is never retried.\n\n`0` is a special value, not \"no wait\": it restores the pre-deadline behavior: the daemon start wait keeps its built-in 45 s budget and target devices are NOT waited for, so an offline target is reported in the send outcome. `--resend` only waits for the daemon.", Default: "15"},
				},
				Args: []*cli.Positional{
					{Name: "TEXT_OR_FILE", Help: "Text or an existing regular file to send. Omit to read from stdin. With `--file`, this is treated as a file path; otherwise omitted stdin is sent as text. Mutually exclusive with `--resend`", ConflictsWith: []string{"resend"}},
				},
				Run: runSend,
			},
			{
				Name:      "watch",
				About:     "Watch inbound clipboard payloads from paired peers and print each delivery as it lands. Press Ctrl-C to stop",
				LongAbout: "Watch inbound clipboard payloads from paired peers and print each delivery as it lands. Press Ctrl-C to stop.\n\nSelf-contained direct mode. Decodes the V3 envelope and shows the first text representation (or a per-rep summary for image-only envelopes). Does NOT write the system clipboard — that's the daemon's job; the CLI watch is purely a diagnostic observer.",
				Run:       runWatch,
			},
			{
				Name:      "get",
				About:     "Get the latest entry now, or wait for the next synced entry",
				LongAbout: "Get the latest entry now, or wait for the next synced entry.\n\nBy default this reads what is already in daemon history and returns immediately. `--wait` waits for one new remote entry and then exits.\n\nSelection (default: the newest usable entry): * `--type <image|file|text|link>` — newest entry of that kind. * `--id <ENTRY-ID>` — a specific entry (see `uniclip search`). * `--list` — list recent entries instead of materializing one.\n\nOutput: text/link content prints to stdout; image/file bytes are written to `--out` (default cache dir) with the absolute path printed to stdout, or streamed to stdout with `--out -`. A successful fetch prints no additional status lines, so the output can be piped directly.\n\nEXIT CODES: `0` materialized; `6` no entry matched the selector; `7` matched but payload unavailable (Lost / not downloaded — re-send from the source device).",
				Flags: []*cli.Flag{
					{Long: "type", ValueName: "KIND", Kind: cli.Enum, Help: "Restrict selection to the newest entry of this kind", PossibleValue: []string{"image", "file", "text", "link"}},
					{Long: "id", ValueName: "ENTRY-ID", Kind: cli.String, Help: "Select a specific entry by id (from `uniclip search`)", ConflictsWith: []string{"type"}},
					{Long: "list", Kind: cli.Bool, Help: "List recent entries instead of materializing one", ConflictsWith: []string{"type", "id"}},
					{Long: "limit", Short: 'n', ValueName: "N", Kind: cli.Uint, Help: "Number of recent entries to scan / list (default 50)"},
					{Long: "out", Short: 'o', ValueName: "DIR|-", Kind: cli.String, Help: "Output for image/file bytes: a directory, or `-` for stdout. Defaults to a per-user cache directory. Ignored for text/link (those always print to stdout)"},
					{Long: "copy", Short: 'c', Kind: cli.Bool, Help: "Copy the result to the clipboard on the computer where this terminal is open", ConflictsWith: []string{"list"}},
					{Long: "wait", Short: 'w', Kind: cli.Bool, Help: "Wait for the next matching remotely synced entry after this command subscribes instead of reading current history. Exits after one match", ConflictsWith: []string{"list", "limit"}},
				},
				Run: runGet,
			},
			{
				Name:             "search",
				About:            "Search clipboard history. Provide a query to search, or use the `status` / `rebuild` subcommands to inspect or maintain the index",
				ArgsConflictSubs: true,
				Flags: []*cli.Flag{
					{Long: "operator", ValueName: "OPERATOR", Kind: cli.String, Help: "Boolean operator: \"and\" or \"or\""},
					{Long: "time-preset", ValueName: "TIME_PRESET", Kind: cli.String, Help: "Time preset: today, yesterday, last_7d, last_30d"},
					{Long: "from-ms", ValueName: "FROM_MS", Kind: cli.Int, Help: "Start of absolute time range, in milliseconds since epoch"},
					{Long: "to-ms", ValueName: "TO_MS", Kind: cli.Int, Help: "End of absolute time range, in milliseconds since epoch"},
					{Long: "type", ValueName: "CONTENT_TYPES", Kind: cli.Strings, Help: "Filter by content type (text, html, file, image, other); repeatable"},
					{Long: "tag", ValueName: "TAGS", Kind: cli.Strings, Help: "Filter by tag (link, favorited, or a custom tag id); repeatable"},
					{Long: "ext", ValueName: "EXTENSIONS", Kind: cli.Strings, Help: "Filter by file extension, for example md or txt; repeatable"},
					{Long: "source-device", ValueName: "SOURCE_DEVICES", Kind: cli.Strings, Help: "Filter by source device — the device a clip arrived from. Accepts a device name (case-insensitive) or a device id; repeatable. Run `uniclip member list` to see paired device names"},
					{Long: "limit", ValueName: "LIMIT", Kind: cli.Uint, Bits: 32, Help: "Maximum results to return", Default: "50"},
					{Long: "offset", ValueName: "OFFSET", Kind: cli.Uint, Bits: 32, Help: "Result offset for pagination", Default: "0"},
					{Long: "detailed", Kind: cli.Bool, Help: "Show detailed metadata for each result"},
				},
				Args: []*cli.Positional{
					{Name: "QUERY", Help: "Free-text query string"},
				},
				Subs: []*cli.Command{
					{
						Name:  "status",
						About: "Show search index status",
						Run:   runSearchStatus,
					},
					{
						Name:  "rebuild",
						About: "Trigger a search index rebuild on the daemon",
						Run:   runSearchRebuild,
					},
				},
			},
			{
				Name:  "upgrade",
				About: "Inspect or advance the upgrade-detection cursor (manual verification for the P1 thin upgrade module). Bare `upgrade` prints status; use the `ack` subcommand to advance the cursor",
				Subs: []*cli.Command{
					{
						Name:  "status",
						About: "Print the upgrade status detected by comparing the persisted version cursor against the current daemon build version",
						Run:   runUpgradeStatus,
					},
					{
						Name:  "ack",
						About: "Advance the version cursor to the current daemon build, marking the upgrade as acknowledged. Idempotent",
						Run:   runUpgradeAck,
					},
				},
			},
			{
				Name:        "debug",
				About:       "Manage persistent local debug logging and export diagnostic logs",
				SubRequired: true,
				Subs: []*cli.Command{
					{
						Name:  "status",
						About: "Show persistent debug-mode status",
						Run:   runDebugStatus,
					},
					{
						Name:  "on",
						About: "Enable persistent debug-mode logging",
						Run:   runDebugOn,
					},
					{
						Name:  "off",
						About: "Disable persistent debug-mode logging",
						Run:   runDebugOff,
					},
					{
						Name:        "capture",
						About:       "Control the daemon-owned detailed connection capture",
						SubRequired: true,
						Subs: []*cli.Command{
							{
								Name:  "status",
								About: "Show the active capture and remaining time",
								Run:   runDebugCaptureStatus,
							},
							{
								Name:  "start",
								About: "Start or reuse one bounded detailed capture",
								Flags: []*cli.Flag{
									{Long: "minutes", ValueName: "MINUTES", Kind: cli.Range, Min: 1, Max: 15, Help: "Capture duration in minutes", Default: "10"},
								},
								Run: runDebugCaptureStart,
							},
							{
								Name:  "stop",
								About: "Stop the matching active capture",
								Args: []*cli.Positional{
									{Name: "CAPTURE_ID", Help: "Capture identifier returned by start or status", Required: true},
								},
								Run: runDebugCaptureStop,
							},
						},
					},
					{
						Name:  "export-logs",
						About: "Export recent GUI, daemon, and CLI logs to Downloads",
						Flags: []*cli.Flag{
							{Long: "since-hours", ValueName: "SINCE_HOURS", Kind: cli.Uint, Bits: 32, Help: "Number of hours to include", Default: "24"},
						},
						Run: runDebugExportLogs,
					},
				},
			},
			mobileTree("mobile", "Manage mobile clipboard sync (iPhone over LAN, SyncClipboard-compatible)", false),
			mobileTree("mobile-sync", "Deprecated alias for `mobile`. Hidden from `--help`; still runs but prints a deprecation notice. Kept so already-published scripts keep working; will be removed in a future release", true),
			{
				Name:   "status",
				Hidden: true,
				About:  "Show application status",
				Run:    runStatus,
			},
			{
				Name:      "init",
				Hidden:    true,
				About:     "Create a new encrypted space for this profile",
				LongAbout: "Create a new encrypted space for this profile.\n\nUse this on the first device before inviting other devices.",
				Flags: []*cli.Flag{
					{Long: "passphrase", ValueName: "PASSPHRASE", Kind: cli.String, Help: "Space passphrase. If omitted, prompts interactively with confirmation. Pass this flag only in non-interactive contexts such as the single-machine e2e test script"},
					{Long: "device-name", ValueName: "DEVICE_NAME", Kind: cli.String, Help: "Display name advertised to paired peers. Defaults to the OS hostname (plus `(profile)` suffix when `--profile` is set)"},
				},
				Run: runInit,
			},
			{
				Name:      "invite",
				Hidden:    true,
				About:     "Issue a pairing invitation and wait for a joiner (sponsor side)",
				LongAbout: "Issue a pairing invitation and wait for a joiner (sponsor side).\n\nSilently resumes the local session from the KEK cached in keychain (or `--dev`'s file secure storage) by a prior `space init` / `unlock` — no passphrase re-entry needed. Fails if the profile has not been initialized yet.",
				Run:       runInvite,
			},
			{
				Name:             "join",
				Hidden:           true,
				About:            "Join a space with an invitation code and passphrase",
				LongAbout:        "Join a space with an invitation code and passphrase.\n\nDefault (re-pair / first-time join) → redeems the invitation and joins the sponsor's space (joiner side of pairing). Safe to run when already in the *same* space: stale member/trust rows are replaced in the new handshake (issue #1023), so this is how you re-pair after a one-sided unpair.\n\n`--switch` → switches to a *different* sponsor's space, re-encrypting local clipboard history under the new master key (4-phase migration: backup → handshake → swap → commit). This is destructive and prompts for confirmation; pass `--yes` to skip the prompt in non-interactive contexts. A daemon crash mid-migration auto-resumes on the next `uniclip` invocation thanks to `MigrationStatePort` persistence.",
				ArgsConflictSubs: true,
				Flags: []*cli.Flag{
					{Long: "code", ValueName: "CODE", Kind: cli.String, Help: "Invitation code printed by the sponsor's `invite`. Prompted interactively when omitted"},
					{Long: "passphrase", ValueName: "PASSPHRASE", Kind: cli.String, Help: "Space passphrase: the sponsor's passphrase when joining, or the new sponsor's passphrase when switching. Prompted interactively when omitted"},
					{Long: "device-name", ValueName: "DEVICE_NAME", Kind: cli.String, Help: "Display name advertised to the sponsor as this device's name on first-time join. Defaults to the OS hostname (plus `(profile)` suffix when `--profile` is set). Persisted to settings before dialing so the B2 handshake can read it back. Ignored with `--switch`"},
					{Long: "switch", Kind: cli.Bool, Help: "Switch to a *different* sponsor's space instead of re-pairing, re-encrypting local clipboard history under the new master key. Destructive; without it `join` always takes the non-destructive re-pair path"},
					{Long: "yes", Kind: cli.Bool, Help: "Skip the confirmation prompt shown before a destructive space switch (re-encrypting local history). Required when switching non-interactively. Only meaningful together with `--switch`"},
					{Long: "preserve-unreadable-history", Kind: cli.Bool, Help: "Keep local history records that cannot be read during a space switch. This requires explicit confirmation and is never implied by `--yes`", Requires: []string{"switch"}},
					{Long: "no-wait", Kind: cli.Bool, Help: "Return after Engine accepts a pending join instead of waiting for a final result"},
				},
				Subs: []*cli.Command{
					{
						Name:  "status",
						About: "Show the current Engine-owned join status",
						Run:   runJoinStatus,
					},
					{
						Name:  "cancel",
						About: "Cancel the current pending join request",
						Run:   runJoinCancel,
					},
				},
			},
			{
				Name:      "members",
				Aliases:   []string{"devices"},
				Hidden:    true,
				About:     "Deprecated alias for `member list`. Hidden from `--help`; still runs with a deprecation warning so existing scripts keep working",
				LongAbout: "Deprecated alias for `member list`. Hidden from `--help`; still runs with a deprecation warning so existing scripts keep working.\n\nSelf-contained direct mode. Prints `{name} ({state}) [local]` per member using each peer's last-known reachability. Pass `--probe` to actively ping every paired peer first so the states are fresh. Also available under the `devices` alias.",
				Flags: []*cli.Flag{
					{Long: "probe", Kind: cli.Bool, Help: "Actively probe paired peers for fresh online/offline state before listing (adds a network round-trip; off by default)"},
				},
				Run: runMembers,
			},
			{
				Name:      "recv",
				Hidden:    true,
				About:     "Deprecated compatibility command. Receive a single inbound file from a paired peer and save it to disk. Exits after the first file arrives (or on Ctrl-C)",
				LongAbout: "Deprecated compatibility command. Receive a single inbound file from a paired peer and save it to disk. Exits after the first file arrives (or on Ctrl-C).\n\nDaemon-client mode: connects to a running daemon (or spawns a transient one), waits for the first inbound clipboard entry that carries a materialized file, exports its bytes from the daemon, and writes them into the output directory. Press Ctrl-C to stop waiting. Does NOT write the system clipboard — recv is strictly a file sink. Progress is shown interactively; on success, stdout contains only the absolute path of the received file.",
				Flags: []*cli.Flag{
					{Long: "out", Short: 'o', ValueName: "DIR", Kind: cli.String, Help: "Output directory. Created if missing. Defaults to current working directory"},
				},
				Run: runRecv,
			},
		},
	}
}

// mobileTree builds `mobile` and its deprecated, hidden `mobile-sync` alias,
// which warns before running the same command.
func mobileTree(name, about string, deprecated bool) *cli.Command {
	wrap := func(run func(*cli.Context) int) func(*cli.Context) int {
		if !deprecated {
			return run
		}
		return func(ctx *cli.Context) int {
			warnMobileSyncAlias()
			return run(ctx)
		}
	}
	return &cli.Command{
		Name:        name,
		Hidden:      deprecated,
		About:       about,
		SubRequired: true,
		Subs: []*cli.Command{
			{
				Name:  "setup",
				About: "One-shot setup wizard: enables the feature, configures the LAN listener, registers an iPhone, and prints the install QR + a one-time password — all in a single command",
				Flags: []*cli.Flag{
					{Long: "label", ValueName: "LABEL", Kind: cli.String, Help: "Human-readable device label, e.g. \"My iPhone 15\". Required in `--non-interactive` / `--json` mode; otherwise prompted"},
					{Long: "ip", ValueName: "IP", Kind: cli.String, Help: "Optional advanced override: pin one LAN IPv4 (e.g. `192.168.1.5`) to the front of the QR's address list. Leave unset and the QR carries every detected LAN interface automatically — the scanning client probes each in turn, so there is normally nothing to pick"},
					{Long: "port", ValueName: "PORT", Kind: cli.Uint, Bits: 16, Help: "Optional advanced override: custom LAN listener port. Leave unset to keep the existing / default port (42720)"},
					{Long: "username", ValueName: "U", Kind: cli.String, Help: "Custom username (6-32 chars, `[A-Za-z0-9_]`, must start with a letter). Leave unset to mint a random `mobile_<8hex>` username"},
					{Long: "password-stdin", Kind: cli.Bool, Help: "Read the password from one line of stdin. Useful for piping from a password manager / CI; stays out of shell history. Mutually exclusive with the interactive prompt"},
					{Long: "accept-network-risk", Kind: cli.Bool, Help: "Accept the network exposure warning non-interactively. **Required** in `--non-interactive` / `--json` mode (no interactive confirmation possible)"},
					{Long: "non-interactive", Kind: cli.Bool, Help: "Skip all interactive prompts. `--label` and `--accept-network-risk` must be given. `--ip` / `--port` / `--username` / `--password-stdin` remain optional (IP/port keep defaults, credentials auto-mint)"},
				},
				Run: wrap(runMobileSetup),
			},
			{
				Name:  "add",
				About: "Pair a new iPhone: mint credentials and print the install QR. Use this to add another phone after the initial `setup`",
				Flags: []*cli.Flag{
					{Long: "label", ValueName: "LABEL", Kind: cli.String, Help: "Human-readable label, e.g. \"My iPhone 15\"", Required: true},
					{Long: "username", ValueName: "U", Kind: cli.String, Help: "Custom username (6-32 chars, `[A-Za-z0-9_]`, letter-leading). Leave unset to mint a random `mobile_<8hex>`"},
					{Long: "password-stdin", Kind: cli.Bool, Help: "Read the password from one line of stdin. Mutually exclusive with auto-mint; both unset → auto-mint"},
				},
				Run: wrap(runMobileAdd),
			},
			{
				Name:  "revoke",
				About: "Unpair an iPhone. Without `<device-id>`, interactively pick from the paired list (JSON mode requires the id explicitly)",
				Args: []*cli.Positional{
					{Name: "DEVICE_ID", Help: "Device id printed by `status` (e.g. `did_<32hex>`)"},
				},
				Run: wrap(runMobileRevoke),
			},
			{
				Name:  "status",
				About: "Combined status view: feature + LAN settings + paired devices + install methods. Daemon-running tolerant",
				Run:   wrap(runMobileStatus),
			},
			{
				Name:  "disable",
				About: "Disable mobile-sync entirely: master switch off + LAN listener off. Paired devices stay registered (use `revoke` to drop them). To stop only the LAN listener, use `network off`",
				Run:   wrap(runMobileDisable),
			},
			{
				Name:        "network",
				About:       "Advanced LAN / reverse-proxy listener configuration. `setup` already handles the common case",
				SubRequired: true,
				Subs: []*cli.Command{
					{
						Name:  "interfaces",
						About: "List eligible RFC1918 LAN IPv4 interfaces (candidates for `network set --ip`)",
						Run:   wrap(runMobileNetworkInterfaces),
					},
					{
						Name:  "set",
						About: "Set the LAN listener address and turn it on (binds 0.0.0.0). Exactly one of --ip / --url decides the address printed in the install URL / QR given to the phone. Re-run to re-point the address or change the port",
						Flags: []*cli.Flag{
							{Long: "ip", ValueName: "IP", Kind: cli.String, Help: "LAN IPv4 to embed in the SyncClipboard install URL (e.g. `192.168.1.5`). Pick one from `network interfaces`. Produces `http://<IP>:<port>`. Mutually exclusive with --url"},
							{Long: "url", ValueName: "URL", Kind: cli.String, Help: "Full base URL (scheme + host + optional port) to embed in the install URL / QR, e.g. `https://clip.example.com`. Use when a TLS reverse proxy (Caddy, nginx, ...) fronts the plain-HTTP LAN listener for public access. Mutually exclusive with --ip"},
							{Long: "port", ValueName: "PORT", Kind: cli.Uint, Bits: 16, Help: "Custom port; default 42720"},
							{Long: "accept-network-risk", Kind: cli.Bool, Help: "Skip the interactive security warning. Required for non-interactive usage (CI / scripts)"},
						},
						Groups: []cli.Group{{Members: []string{"ip", "url"}, Required: true}},
						Run:    wrap(runMobileNetworkSet),
					},
					{
						Name:  "off",
						About: "Turn off just the LAN listener (master switch and paired devices stay; use top-level `disable` to take mobile-sync fully offline)",
						Run:   wrap(runMobileNetworkOff),
					},
				},
			},
		},
	}
}
