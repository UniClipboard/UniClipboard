package main

import (
	"fmt"
	"strings"

	"github.com/uniclipboard/unic/engine"
)

// usageErr marks CLI syntax or usage errors so main exits with code 2.
type usageErr struct{ msg string }

func (e *usageErr) Error() string { return e.msg }

func usageError(msg string) error { return &usageErr{msg: msg} }

// silentExitError returns a specific exit code without printing another error line.
type silentExitError struct{ code int }

func (e *silentExitError) Error() string { return "" }

// wantsHelp reports whether args contain -h or --help before "--".
func wantsHelp(args []string) bool {
	for _, a := range args {
		if a == "--" {
			return false
		}
		if a == "-h" || a == "--help" {
			return true
		}
	}
	return false
}

// capabilitiesSummary is a human- and JSON-friendly summary of Engine capabilities.
type capabilitiesSummary struct {
	Observe string `json:"observe"`
	Apply   string `json:"apply"`
	Summary string `json:"summary"`
}

func capabilitiesView(c engine.Capabilities) capabilitiesSummary {
	obs := string(c.Effective.Observe)
	app := string(c.Effective.Apply)
	var summary string
	switch {
	case c.Effective.Observe == engine.AbilityAvailable && c.Effective.Apply == engine.AbilityAvailable:
		summary = "available (observe + apply)"
	case c.Effective.Observe == engine.AbilityAvailable:
		summary = "observe only (apply: " + app + ")"
	case c.Effective.Apply == engine.AbilityAvailable:
		summary = "apply only (observe: " + obs + ")"
	case obs == "unavailable" && app == "unavailable":
		summary = "unavailable (headless)"
	default:
		summary = fmt.Sprintf("observe=%s, apply=%s", obs, app)
	}
	return capabilitiesSummary{Observe: obs, Apply: app, Summary: summary}
}

const helpExitCodesText = `Exit codes:
  0   Done
  1   Operation failed
  2   Usage or argument error
  3   v1 migration partially completed
  4   Committed locally, but send/pairing/stream did not complete
  5   Command finished, but temporary unicd cleanup failed
  6   Profile not initialized (run ` + "`unic init`" + ` or ` + "`unic migrate`" + `)
`

const sendUsage = `Send text, files, directories, or a PNG image.

Usage:
  unic send [flags] [text]
  unic s [flags] [text]

Modes:
  Without --to        Create a temporary one-time transfer (6-digit code + link)
  With --to all|DEV   Sync directly to all paired devices or one named device/ID

Flags:
  --to TARGET         Target paired device (all, device name, or device ID)
  -f                  Send one or more file or directory paths (directories require --to)
  --png PATH          Send a PNG image file
  --request-id KEY    Idempotency key for scripted retries (with --to)
  --json              Output result as JSON
  -h, --help          Show this help

Examples:
  unic send "hello"                       Share once with a 6-digit code
  echo "hi" | unic send --to all          Pipe stdin to all paired devices
  unic send --to laptop -f report.pdf     Send a file to a specific device
  unic send --to all --png shot.png       Send a PNG image
`

const recvUsage = `Receive content from a transfer code/link, or inspect transient received items.

Usage:
  unic recv <code|link> [--json]
  unic recv [list] [--json]
  unic recv show <id> [part]
  unic recv dismiss <id>

Aliases:
  r

Examples:
  unic recv 482913                        Receive from a 6-digit transfer code
  unic recv "https://try.uniclipboard.app/#..."
  unic recv list                          List transient received items
`

const statusUsage = `Show active profile, daemon state, device identity, and paired peers.

Usage:
  unic status [--json]

Behavior:
  Probes the local socket without starting a temporary unicd.
  When the daemon is stopped, prints profile and daemon status immediately.

Flags:
  --json       Output status as JSON
  -h, --help   Show this help
`

const deviceUsage = `Pair, approve, and manage trusted devices.

Usage:
  unic device <command> [flags]

Commands:
  add [CODE|ADDRESS] [--backend ID]     Show a pairing code, or pair with a code/address
  list [--refresh] [--json]             List paired and waiting devices
  addr [--backend ID]                   Print this device's pairing address (secret)
  approve <id> <fingerprint>            Approve a waiting device after verifying fingerprint
  reject <id>                           Reject a waiting device
  rename <id> [NAME] [--clear] [--json] Set or clear a custom display name for a device
  remove <id>                           Forget a paired device
  path pause|resume|remove|policy ...   Manage one sync path binding
  backends [--json]                     List configured sync backends
  suggest [list|pair|ignore|restore]    Manage devices vouched for by paired devices

Examples:
  unic device add                       Show a 6-digit pairing code
  unic device add 123456                Pair using a code from another device
  unic device list --refresh            Refresh peer connectivity and list devices
  unic device rename <id> "Work Mac"    Rename a paired device
`

const channelUsage = `Pair and manage external clipboard channels.

Usage:
  unic channel <command> [flags]

Commands:
  pair <instance> [--resume] [--json]   Pair an external clipboard channel
  add <instance> <name> [--json]        Register a named local channel on an instance
  pause <id> <revision> [--json]        Pause syncing for a channel
  resume <id> <revision> [--json]       Resume syncing for a channel
  remove <id> <revision>                Remove an external channel

Examples:
  unic channel pair wechat.sync
  unic channel pair wechat.sync --resume
`

const configUsage = `View and update profile configuration (config.toml).

Usage:
  unic config [--json]                  Print effective configuration
  unic config path                      Print config.toml file path (local)
  unic config set <key> <TOML-value>    Update one setting (validated & saved)
  unic config apply <file>              Replace config.toml from a file

Examples:
  unic config path
  unic config set device_name '"MacBook Pro"'
  unic config set sync.directories.automatic true
`

const devUsage = `Development, inspection, and diagnostic commands.

Usage:
  unic dev <command> [flags]

Commands:
  watch                                 Stream live daemon events
  capabilities                          Print Engine clipboard capabilities (JSON)
  submission <request-id>               Inspect a recent explicit submission (JSON)
  cache [--json]                        Inspect managed file cache state
  logs level [LEVEL] [--target ...]     Inspect or temporarily override log level
  logs export [--output FILE]           Export local log archive offline
  diagnostics telemetry                 Inspect local telemetry status (JSON)
  copy [TEXT | -f FILE...]              Write to system clipboard via desktop adapter
  paste                                 Read system clipboard via desktop adapter
`

func helpForCommand(name string) (string, bool) {
	switch name {
	case "init":
		return initUsage, true
	case "daemon", "daemon help":
		return daemonUsage, true
	case "profile", "profile help":
		return profileUsage, true
	case "cloud", "cloud help", "cloud login", "cloud status", "cloud logout":
		return cloudUsage, true
	case "doctor":
		return doctorUsage, true
	case "completion":
		return completionUsage, true
	case "send":
		return sendUsage, true
	case "recv":
		return recvUsage, true
	case "status":
		return statusUsage, true
	case "device", "device help":
		return deviceUsage, true
	case "channel", "channel help":
		return channelUsage, true
	case "config":
		return configUsage, true
	case "history", "history help":
		return historyUsage, true
	case "plugin":
		return pluginUsage, true
	case "migrate":
		return migrateUsage, true
	case "dev", "dev help":
		return devUsage, true
	case "dev logs", "dev logs help":
		return "Usage:\n  unic dev logs level [debug|info|warn|error|reset] [--target local|remote] [--for 15m]\n  " + strings.TrimPrefix(logExportUsage, "usage: ") + "\n", true
	}
	if strings.HasPrefix(name, "daemon ") {
		sub := strings.TrimPrefix(name, "daemon ")
		var b strings.Builder
		switch sub {
		case "start":
			b.WriteString("Usage: unic daemon start [--foreground] [--json]\n\nStart the background unicd service for the active profile.\n")
		case "stop":
			b.WriteString("Usage: unic daemon stop [--json]\n\nStop the running standalone unicd service for the active profile.\n")
		case "restart":
			b.WriteString("Usage: unic daemon restart [--json]\n\nRestart the standalone unicd service.\n")
		case "status":
			b.WriteString("Usage: unic daemon status [--json]\n\nInspect daemon state without starting a temporary service.\n")
		case "logs":
			b.WriteString("Usage: unic daemon logs [-n LINES] [-f|--follow]\n\nPrint the tail of the daemon log file.\n")
		default:
			return daemonUsage, true
		}
		return b.String(), true
	}
	if strings.HasPrefix(name, "device ") {
		return deviceUsage, true
	}
	if strings.HasPrefix(name, "channel ") {
		return channelUsage, true
	}
	if strings.HasPrefix(name, "history ") {
		return historyUsage, true
	}
	if strings.HasPrefix(name, "plugin ") {
		return pluginUsage, true
	}
	if strings.HasPrefix(name, "profile ") {
		return profileUsage, true
	}
	if strings.HasPrefix(name, "dev ") {
		return devUsage, true
	}
	return "", false
}

func formatUsageLine(s string) string {
	return strings.TrimRight(s, "\n")
}
