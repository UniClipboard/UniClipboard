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
	reqLocal            requirement = "local"             // never contacts unicd
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
	{Name: "config path", Requirement: reqLocal, Completion: doneNone},
	{Name: "dev help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "dev logs export", Requirement: reqLocal, Completion: doneNone},
	{Name: "dev logs help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "plugin", Requirement: reqLocal, Completion: doneNone},
	{Name: "history help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "migrate", Requirement: reqOffline, Completion: doneNone},

	{Name: "send", Requirement: reqService, Completion: doneSend},
	{Name: "recv", Requirement: reqService, Completion: doneResponse},
	{Name: "status", Requirement: reqService, Completion: doneResponse},
	{Name: "config", Requirement: reqService, Completion: doneResponse},

	{Name: "device add", Requirement: reqService, Completion: doneSession},
	{Name: "device addr", Requirement: reqService, Completion: doneResponse},
	{Name: "device list", Requirement: reqService, Completion: doneResponse},
	{Name: "device remove", Requirement: reqService, Completion: doneResponse},
	{Name: "device approve", Requirement: reqService, Completion: doneResponse},
	{Name: "device reject", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggestions", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-pair", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-ignore", Requirement: reqService, Completion: doneResponse},
	{Name: "device suggest-restore", Requirement: reqService, Completion: doneResponse},

	{Name: "history list", Aliases: []string{"ls"}, Requirement: reqService, Completion: doneResponse},
	{Name: "history show", Requirement: reqService, Completion: doneResponse},
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

func lookup(name string) leaf {
	for _, l := range catalog {
		if l.Name == name {
			return l
		}
	}
	panic("uncatalogued command leaf: " + name)
}

// firstWord maps a typed first word to its command. These are the only aliases of command words.
var firstWord = map[string]string{"s": "send", "r": "recv", "h": "history", "-h": "help", "--help": "help"}

// groupUsage is printed when a group has no valid subcommand.
var groupUsage = map[string]string{
	"device": "usage: unic device add [CODE|ADDRESS] | addr | list | remove ID | approve ID FINGERPRINT | reject ID | suggestions | suggest-pair ID BACKEND | suggest-ignore ID | suggest-restore ID",
	"dev":    "usage: unic dev submission ID | capabilities | watch | logs level|export|help | diagnostics telemetry | cache [--json] | copy [TEXT] | paste",
}

// classify resolves a command line to its declaration before any service is touched.
// It returns the leaf and the arguments after the words that name it. It rejects unknown
// commands and subcommands; deeper argument syntax is checked by the handler before its
// first request, and a request is what acquires the service.
func classify(cmd string, args []string) (leaf, []string, error) {
	if cmd == "daemon" {
		return leaf{}, nil, fmt.Errorf("the daemon is a separate program: run `unicd`")
	}
	if to, ok := firstWord[cmd]; ok {
		cmd = to
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
	case "history":
		if len(args) == 0 {
			return leaf{}, nil, fmt.Errorf("%s", strings.TrimRight(historyUsage, "\n"))
		}
		return leaf{}, nil, fmt.Errorf("unknown history command %q\n%s", args[0], strings.TrimRight(historyUsage, "\n"))
	case "device", "dev":
		return leaf{}, nil, fmt.Errorf("%s", groupUsage[cmd])
	case "submission", "capabilities", "watch", "logs", "diagnostics", "cache", "copy", "paste":
		return leaf{}, nil, fmt.Errorf("%q is a development command: run `unic dev %s`", cmd, cmd)
	}
	return leaf{}, nil, fmt.Errorf("unknown command %q (try `unic help`)", cmd)
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

// helpLine is one summary line. Badges come from the leaf declarations named in Leaves.
type helpLine struct {
	Usage, Text string
	Leaves      []string
}

var helpLines = []helpLine{
	{"send, s [flags] [text]", "send text, stdin, -f files, or --png PATH to your devices", []string{"send"}},
	{"recv, r", "list, show or dismiss received content", []string{"recv"}},
	{"history, h", "list, show, delete and tag clipboard history (unic h help)", []string{"history list", "history use"}},
	{"device add [code|address]", "pair a device: without an argument, show a 6-digit code for the other device", []string{"device add"}},
	{"device addr", "print this device's address (treat it like a password)", []string{"device addr"}},
	{"device list", "list paired devices and devices waiting for your approval", []string{"device list"}},
	{"device remove <id>", "forget a paired device", []string{"device remove"}},
	{"device approve <id> <fingerprint>", "accept after checking the other device", []string{"device approve"}},
	{"device reject <id>", "dismiss a waiting device", []string{"device reject"}},
	{"device suggestions", "devices your paired devices vouched for (suggest-pair|suggest-ignore|suggest-restore)", []string{"device suggestions", "device suggest-pair", "device suggest-ignore", "device suggest-restore"}},
	{"status", "show this device and its peers", []string{"status"}},
	{"config", "show settings; set <key> <TOML-value>; apply <file>; --json", []string{"config"}},
	{"config path", "print the settings file path", []string{"config path"}},
	{"plugin", "list local plugins; enable or disable one (unic plugin help)", []string{"plugin"}},
	{"migrate [flags]", "import local UniClipboard v1 history", []string{"migrate"}},
	{"dev", "development and diagnosis (unic dev help)", []string{"dev watch", "dev copy", "dev logs export"}},
}

func badge(names []string) string {
	seen := map[requirement]bool{}
	var out []string
	for _, n := range names {
		r := lookup(n).Requirement
		if !seen[r] {
			seen[r] = true
			out = append(out, string(r))
		}
	}
	return "[" + strings.Join(out, "/") + "]"
}

func usageText() string {
	var b strings.Builder
	b.WriteString("unic - clipboard sync between your devices\n\nUsage: unic <command>\n\n")
	b.WriteString("Commands marked [service] use the running unicd. When none runs, unic starts a temporary\n")
	b.WriteString("one and stops it after the last command finishes. A running unicd or desktop is never stopped.\n")
	b.WriteString("[local] commands never contact unicd. [offline] commands need exclusive access to their profile.\n")
	b.WriteString("[service+clipboard] commands also need the desktop clipboard adapter; unic never starts a GUI.\n\n")
	for _, l := range helpLines {
		fmt.Fprintf(&b, "  %-28s %s %s\n", l.Usage, badge(l.Leaves), l.Text)
	}
	b.WriteString(`
A temporary unicd lives only while commands use it. Commands that wait for another device
(device add, dev logs level --for) keep it running for a bounded window and say so on stderr.
To keep receiving content or to keep a code usable after the command ends, run unicd yourself.
Exit codes: 0 done, 1 failed, 2 usage, 3 migration partial, 4 committed but not fully completed
(send, approval or window incomplete), 5 done but temporary unicd cleanup failed.

Settings live in config.toml in the state directory; saving it applies it at once.
Set UNIC_HOME to run a separate profile, UNIC_RENDEZVOUS_URL to use another
pairing code service.
`)
	return b.String()
}
