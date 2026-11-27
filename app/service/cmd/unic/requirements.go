package main

import (
	"encoding/json"
	"fmt"
	"io"
	"slices"
	"strings"
)

// requirement says what a leaf command needs before it runs.
type requirement string

const (
	reqLocal            requirement = "local"             // never starts temporary unicd
	reqService          requirement = "service"           // reuses or temporarily starts unicd
	reqServiceClipboard requirement = "service+clipboard" // as service, plus a desktop clipboard adapter
	reqOffline          requirement = "offline"           // needs its target profile without a running owner
)

// completion says when a command is done and releases its usage.
type completion string

const (
	doneNone     completion = "none"     // no service work
	doneResponse completion = "response" // after the response or stream is consumed
	doneStream   completion = "stream"   // until the user stops the stream
	doneSend     completion = "send"     // after this command's send operation ends
	doneSession  completion = "session"  // a temporary service is held for a bounded window
)

// leaf is one valid command shape. Every leaf, alias and conditional flag appears here.
type leaf struct {
	Name        string      `json:"leaf"`
	Aliases     []string    `json:"aliases,omitempty"`
	Requirement requirement `json:"requirement"`
	Completion  completion  `json:"completion"`
	// Conditional names a flag that raises the requirement to service+clipboard.
	Conditional string `json:"conditional_flag,omitempty"`
}

var catalog = []leaf{
	{Name: "help", Requirement: reqLocal, Completion: doneNone},
	{Name: "init", Requirement: reqOffline, Completion: doneNone},
	{Name: "migrate", Requirement: reqOffline, Completion: doneNone},
	{Name: "status", Requirement: reqLocal, Completion: doneNone},
	{Name: "doctor", Requirement: reqLocal, Completion: doneNone},
	{Name: "completion", Requirement: reqLocal, Completion: doneNone},

	{Name: "daemon start", Requirement: reqLocal, Completion: doneNone},
	{Name: "daemon stop", Requirement: reqLocal, Completion: doneNone},
	{Name: "daemon restart", Requirement: reqLocal, Completion: doneNone},
	{Name: "daemon status", Requirement: reqLocal, Completion: doneNone},
	{Name: "daemon logs", Requirement: reqLocal, Completion: doneNone},
	{Name: "daemon help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},

	{Name: "profile list", Aliases: []string{"ls"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "profile path", Requirement: reqLocal, Completion: doneNone},
	{Name: "profile help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "profile", Requirement: reqLocal, Completion: doneNone},

	{Name: "config path", Requirement: reqLocal, Completion: doneNone},
	{Name: "cloud login", Requirement: reqLocal, Completion: doneNone},
	{Name: "cloud status", Requirement: reqLocal, Completion: doneNone},
	{Name: "cloud logout", Requirement: reqLocal, Completion: doneNone},
	{Name: "cloud help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "dev help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "dev logs export", Requirement: reqLocal, Completion: doneNone},
	{Name: "dev logs help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "plugin settings show", Requirement: reqService, Completion: doneResponse},
	{Name: "plugin settings save", Requirement: reqService, Completion: doneResponse},
	{Name: "plugin", Requirement: reqLocal, Completion: doneNone},
	{Name: "history help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "device help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "channel help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},

	{Name: "send", Requirement: reqService, Completion: doneSend},
	{Name: "recv", Requirement: reqService, Completion: doneResponse},
	{Name: "config", Requirement: reqService, Completion: doneResponse},

	{Name: "channel pair", Requirement: reqService, Completion: doneResponse},
	{Name: "channel add", Requirement: reqService, Completion: doneResponse},
	{Name: "channel pause", Requirement: reqService, Completion: doneResponse},
	{Name: "channel resume", Requirement: reqService, Completion: doneResponse},
	{Name: "channel remove", Requirement: reqService, Completion: doneResponse},

	{Name: "device add", Requirement: reqService, Completion: doneSession},
	{Name: "device addr", Requirement: reqService, Completion: doneResponse},
	{Name: "device path", Requirement: reqService, Completion: doneResponse},
	{Name: "device backends", Requirement: reqService, Completion: doneResponse},
	{Name: "device list", Aliases: []string{"ls"}, Requirement: reqService, Completion: doneResponse},
	{Name: "device rename", Requirement: reqService, Completion: doneResponse},
	{Name: "device remove", Aliases: []string{"rm"}, Requirement: reqService, Completion: doneResponse},
	{Name: "device approve", Requirement: reqService, Completion: doneResponse},
	{Name: "device reject", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggestions", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-pair", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-ignore", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-restore", Requirement: reqService, Completion: doneResponse},
	{Name: "device channel-add", Requirement: reqService, Completion: doneResponse},
	{Name: "device channel", Requirement: reqService, Completion: doneResponse},

	{Name: "history list", Aliases: []string{"ls"}, Requirement: reqService, Completion: doneResponse},
	{Name: "history show", Requirement: reqService, Completion: doneResponse},
	{Name: "history send", Requirement: reqService, Completion: doneSend},
	{Name: "history export", Requirement: reqService, Completion: doneResponse},
	{Name: "history delete", Aliases: []string{"rm"}, Requirement: reqService, Completion: doneResponse},
	{Name: "history tag", Requirement: reqService, Completion: doneResponse},
	{Name: "history expire", Requirement: reqService, Completion: doneResponse},
	{Name: "history action", Requirement: reqService, Completion: doneSend, Conditional: "--apply"},
	{Name: "history use", Requirement: reqServiceClipboard, Completion: doneSend},

	{Name: "dev submission", Requirement: reqService, Completion: doneResponse},
	{Name: "dev capabilities", Requirement: reqService, Completion: doneResponse},
	{Name: "dev watch", Requirement: reqService, Completion: doneStream},
	{Name: "dev logs level", Requirement: reqService, Completion: doneSession},
	{Name: "dev diagnostics telemetry", Requirement: reqService, Completion: doneResponse},
	{Name: "dev cache", Requirement: reqService, Completion: doneResponse},
	{Name: "dev copy", Requirement: reqServiceClipboard, Completion: doneResponse},
	{Name: "dev paste", Requirement: reqServiceClipboard, Completion: doneResponse},
}

// firstWord maps a typed first word to its command.
var firstWord = map[string]string{"s": "send", "r": "recv", "h": "history", "-h": "help", "--help": "help"}

// groupUsage is printed when a group has no valid subcommand.
var groupUsage = map[string]string{
	"daemon":  formatUsageLine(daemonUsage),
	"channel": formatUsageLine(channelUsage),
	"device":  formatUsageLine(deviceUsage),
	"dev":     formatUsageLine(devUsage),
	"history": formatUsageLine(historyUsage),
	"profile": formatUsageLine(profileUsage),
	"cloud":   formatUsageLine(cloudUsage),
}

// classify resolves a command line to its declaration before any service is touched.
// It returns the leaf and the arguments after the words that name it. It rejects unknown
// commands and subcommands; deeper argument syntax is checked by the handler before its
// first request, and a request is what acquires the service.
func classify(cmd string, args []string) (leaf, []string, error) {
	if to, ok := firstWord[cmd]; ok {
		cmd = to
	}
	// Any command/group with -h or --help is handled locally before init/service acquisition.
	if cmd != "help" && wantsHelp(args) {
		if len(args) > 0 && args[0] != "-h" && args[0] != "--help" {
			words := append([]string{cmd}, args...)
			for n := min(3, len(words)); n >= 1; n-- {
				if l, ok := match(words[:n]); ok {
					return leaf{Name: l.Name, Requirement: reqLocal, Completion: doneNone}, []string{"--help"}, nil
				}
			}
		}
		if _, ok := helpForCommand(cmd); ok {
			return leaf{Name: cmd, Requirement: reqLocal, Completion: doneNone}, []string{"--help"}, nil
		}
	}
	words := append([]string{cmd}, args...)
	for n := min(3, len(words)); n >= 1; n-- {
		l, ok := match(words[:n])
		if !ok {
			continue
		}
		if l.Conditional != "" && hasFlag(words[n:], l.Conditional) {
			l.Requirement = reqServiceClipboard
		}
		return l, words[n:], nil
	}
	switch cmd {
	case "daemon", "history", "channel", "device", "dev":
		if len(args) == 0 {
			return leaf{}, nil, usageError(groupUsage[cmd])
		}
		return leaf{}, nil, usageError(fmt.Sprintf("unknown %s command %q\n\n%s", cmd, args[0], groupUsage[cmd]))
	case "submission", "capabilities", "watch", "logs", "diagnostics", "cache", "copy", "paste":
		return leaf{}, nil, usageError(fmt.Sprintf("%q is a development command: run `unic dev %s`", cmd, cmd))
	}
	return leaf{}, nil, usageError(fmt.Sprintf("unknown command %q (try `unic help`)", cmd))
}

// match finds the leaf named by exactly these words. A leaf alias replaces its last word.
func match(words []string) (leaf, bool) {
	name := strings.Join(words, " ")
	head := strings.Join(words[:len(words)-1], " ")
	for _, l := range catalog {
		if l.Name == name {
			return l, true
		}
		if len(words) > 1 && strings.HasPrefix(l.Name, head+" ") && !strings.Contains(strings.TrimPrefix(l.Name, head+" "), " ") && slices.Contains(l.Aliases, words[len(words)-1]) {
			return l, true
		}
	}
	return leaf{}, false
}

// hasFlag reports an explicit boolean flag, ignoring an explicit false.
func hasFlag(args []string, flag string) bool {
	for _, a := range args {
		if a == "--" {
			return false
		}
		if a == flag || a == flag+"=true" || a == flag+"=1" || a == "-"+strings.TrimPrefix(flag, "--") {
			return true
		}
	}
	return false
}

// writeCatalog prints the machine-readable declaration used for help and coverage checks.
func writeCatalog(w io.Writer) error {
	enc := json.NewEncoder(w)
	enc.SetIndent("", "  ")
	return enc.Encode(catalog)
}

type helpGroup struct {
	Title string
	Items []helpItem
}

type helpItem struct {
	Usage string
	Text  string
}

var helpGroups = []helpGroup{
	{
		Title: "Getting Started",
		Items: []helpItem{
			{"init", "Initialize a profile for first use"},
			{"status", "Show profile, daemon, and peer status"},
			{"daemon", "Start, stop, and inspect the background service"},
		},
	},
	{
		Title: "Clipboard & Sync",
		Items: []helpItem{
			{"send, s", "Send text, files, or images (code/link or --to DEVICE)"},
			{"recv, r", "Receive from a code/link, or list received items"},
			{"history, h", "Browse, show, restore, tag, and export history"},
		},
	},
	{
		Title: "Devices & Extensions",
		Items: []helpItem{
			{"device", "Pair, approve, and manage devices"},
			{"config", "View and update settings"},
			{"plugin", "Install and manage plugins"},
			{"migrate", "Import UniClipboard v1 history"},
		},
	},
	{
		Title: "Advanced",
		Items: []helpItem{
			{"profile", "List and inspect local profiles"},
			{"channel", "Pair and manage external clipboard channels"},
			{"doctor", "Run diagnostics on the active profile and daemon"},
			{"completion", "Generate shell completion scripts (bash, zsh, fish)"},
			{"dev", "Diagnostics, logs, and low-level tools"},
		},
	},
}

func usageText() string {
	var b strings.Builder
	b.WriteString(style(outTTY, sBold, "unic") + " - peer-to-peer clipboard sync for your devices\n\n")
	b.WriteString(style(outTTY, sBold, "Usage:") + "\n  unic [global flags] <command> [flags]\n")
	for _, g := range helpGroups {
		b.WriteString("\n" + style(outTTY, sBold, g.Title+":") + "\n")
		for _, it := range g.Items {
			pad := strings.Repeat(" ", max(2, 16-len(it.Usage)))
			fmt.Fprintf(&b, "  %s%s%s\n", style(outTTY, sCmd, it.Usage), pad, it.Text)
		}
	}
	b.WriteString("\n" + style(outTTY, sBold, "Global Flags:") + "\n")
	b.WriteString("  -p, --profile NAME   Use a named profile (env: UNIC_PROFILE)\n")
	b.WriteString("      --json           Output machine-readable JSON where supported\n")
	b.WriteString("  -h, --help           Show help for any command\n")
	b.WriteString("      --version        Print version information\n")
	b.WriteString("\n" + style(outTTY, sBold, "Examples:") + "\n")
	b.WriteString("  unic init                          Initialize default profile\n")
	b.WriteString("  unic daemon start                  Keep syncing in the background\n")
	b.WriteString("  unic send \"hello\"                  Share once with a 6-digit code\n")
	b.WriteString("  unic device add                    Pair another device\n")
	b.WriteString("\nRun `unic <command> --help` for details, or `unic help exit-codes` for exit codes.\n")
	return b.String()
}
