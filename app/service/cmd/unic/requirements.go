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
	{Name: "help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "config path", Requirement: reqLocal, Completion: doneNone},
	{Name: "logs export", Requirement: reqLocal, Completion: doneNone},
	{Name: "logs help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "plugin", Requirement: reqLocal, Completion: doneNone},
	{Name: "history help", Aliases: []string{"-h", "--help"}, Requirement: reqLocal, Completion: doneNone},
	{Name: "migrate", Requirement: reqOffline, Completion: doneNone},

	{Name: "submit", Requirement: reqService, Completion: doneSend},
	{Name: "submission", Requirement: reqService, Completion: doneResponse},
	{Name: "received", Requirement: reqService, Completion: doneResponse},
	{Name: "capabilities", Requirement: reqService, Completion: doneResponse},
	{Name: "status", Requirement: reqService, Completion: doneResponse},
	{Name: "peers", Requirement: reqService, Completion: doneResponse},
	{Name: "addr", Requirement: reqService, Completion: doneResponse},
	{Name: "code", Requirement: reqService, Completion: doneSession},
	{Name: "join", Requirement: reqService, Completion: doneSession},
	{Name: "pair", Requirement: reqService, Completion: doneSession},
	{Name: "unpair", Requirement: reqService, Completion: doneResponse},
	{Name: "requests", Requirement: reqService, Completion: doneResponse},
	{Name: "approve", Requirement: reqService, Completion: doneResponse},
	{Name: "reject", Requirement: reqService, Completion: doneResponse},
	{Name: "suggestions", Requirement: reqService, Completion: doneResponse},
	{Name: "suggest-pair", Requirement: reqService, Completion: doneResponse},
	{Name: "suggest-ignore", Requirement: reqService, Completion: doneResponse},
	{Name: "suggest-restore", Requirement: reqService, Completion: doneResponse},
	{Name: "watch", Requirement: reqService, Completion: doneStream},
	{Name: "config", Requirement: reqService, Completion: doneResponse},
	{Name: "logs level", Requirement: reqService, Completion: doneSession},
	{Name: "diagnostics telemetry", Requirement: reqService, Completion: doneResponse},
	{Name: "cache", Requirement: reqService, Completion: doneResponse},
	{Name: "history list", Aliases: []string{"ls"}, Requirement: reqService, Completion: doneResponse},
	{Name: "history show", Requirement: reqService, Completion: doneResponse},
	{Name: "history export", Requirement: reqService, Completion: doneResponse},
	{Name: "history delete", Aliases: []string{"rm"}, Requirement: reqService, Completion: doneResponse},
	{Name: "history tag", Requirement: reqService, Completion: doneResponse},
	{Name: "history expire", Requirement: reqService, Completion: doneResponse},
	{Name: "history action", Requirement: reqService, Completion: doneSend, Conditional: "--apply"},

	{Name: "copy", Requirement: reqServiceClipboard, Completion: doneResponse},
	{Name: "paste", Requirement: reqServiceClipboard, Completion: doneResponse},
	{Name: "history use", Requirement: reqServiceClipboard, Completion: doneSend},
}

func lookup(name string) leaf {
	for _, l := range catalog {
		if l.Name == name {
			return l
		}
	}
	panic("uncatalogued command leaf: " + name)
}

// classify resolves a command line to its declaration before any service is touched.
// It rejects unknown commands and subcommands; deeper argument syntax is checked by the
// handler before its first request, and a request is what acquires the service.
func classify(cmd string, args []string) (leaf, error) {
	switch cmd {
	case "help", "-h", "--help":
		return lookup("help"), nil
	case "daemon":
		return leaf{}, fmt.Errorf("the daemon is a separate program: run `unicd`")
	case "migrate":
		return lookup("migrate"), nil
	case "plugin":
		return lookup("plugin"), nil
	case "history", "h":
		return classifyHistory(args)
	case "config":
		if len(args) == 1 && args[0] == "path" {
			return lookup("config path"), nil
		}
		return lookup("config"), nil
	case "logs":
		if len(args) > 0 {
			switch args[0] {
			case "level":
				return lookup("logs level"), nil
			case "export":
				return lookup("logs export"), nil
			case "help", "--help", "-h":
				return lookup("logs help"), nil
			}
		}
		return leaf{}, fmt.Errorf("usage: unic logs level ... | unic logs export [--output FILE]")
	case "diagnostics":
		if len(args) != 1 || args[0] != "telemetry" {
			return leaf{}, fmt.Errorf("usage: unic diagnostics telemetry")
		}
		return lookup("diagnostics telemetry"), nil
	}
	for _, l := range catalog {
		if l.Name == cmd && !strings.Contains(l.Name, " ") {
			return l, nil
		}
	}
	return leaf{}, fmt.Errorf("unknown command %q (try `unic help`)", cmd)
}

func classifyHistory(args []string) (leaf, error) {
	if len(args) == 0 {
		return leaf{}, fmt.Errorf("%s", strings.TrimRight(historyUsage, "\n"))
	}
	sub := args[0]
	if sub == "help" || sub == "-h" || sub == "--help" {
		return lookup("history help"), nil
	}
	for _, l := range catalog {
		name, ok := strings.CutPrefix(l.Name, "history ")
		if !ok || (name != sub && !slices.Contains(l.Aliases, sub)) {
			continue
		}
		if l.Conditional != "" && hasFlag(args[1:], l.Conditional) {
			l.Requirement = reqServiceClipboard
		}
		return l, nil
	}
	return leaf{}, fmt.Errorf("unknown history command %q\n%s", sub, strings.TrimRight(historyUsage, "\n"))
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
	{"submit [flags] [text]", "submit text, stdin, -f files, or --png PATH without a clipboard", []string{"submit"}},
	{"submission ID", "query a submission by request_id", []string{"submission"}},
	{"received", "list, show or dismiss transient received content", []string{"received"}},
	{"capabilities", "show system clipboard capabilities", []string{"capabilities"}},
	{"status", "show this device and its peers", []string{"status"}},
	{"addr", "print this device's address (treat it like a password)", []string{"addr"}},
	{"code", "show a 6-digit code another device can join with", []string{"code"}},
	{"join <code>", "pair with the device showing that code", []string{"join"}},
	{"pair <address>", "pair with another device by its full address", []string{"pair"}},
	{"peers", "list paired devices", []string{"peers"}},
	{"unpair <id>", "forget a paired device", []string{"unpair"}},
	{"requests", "devices waiting for your approval", []string{"requests"}},
	{"approve <id> <fingerprint>", "accept after checking the other device", []string{"approve"}},
	{"reject <id>", "dismiss a waiting device", []string{"reject"}},
	{"suggestions", "devices your paired devices vouched for (suggest-pair|suggest-ignore|suggest-restore)", []string{"suggestions", "suggest-pair", "suggest-ignore", "suggest-restore"}},
	{"history, h", "list, show, delete and tag clipboard history (unic h help)", []string{"history list", "history use"}},
	{"copy [text]", "put text (or stdin, or -f files) on the clipboard", []string{"copy"}},
	{"paste", "print the clipboard's text", []string{"paste"}},
	{"watch", "print sync events as they happen", []string{"watch"}},
	{"config", "show settings; set <key> <TOML-value>; apply <file>; --json", []string{"config"}},
	{"config path", "print the settings file path", []string{"config path"}},
	{"logs level", "query or temporarily override local/remote log levels", []string{"logs level"}},
	{"logs export [--output FILE]", "archive retained local logs", []string{"logs export"}},
	{"diagnostics telemetry", "show telemetry state and dropped records", []string{"diagnostics telemetry"}},
	{"cache [--json]", "show default-cache bytes, reservations, and protected files", []string{"cache"}},
	{"plugin", "list local plugins; enable or disable one (unic plugin help)", []string{"plugin"}},
	{"migrate [flags]", "import local UniClipboard v1 history", []string{"migrate"}},
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
(pair, join, code, logs level --for) keep it running for a bounded window and say so on stderr.
To keep receiving content or to keep a code usable after the command ends, run unicd yourself.
Exit codes: 0 done, 1 failed, 2 usage, 3 migration partial, 4 committed but not fully completed
(send, approval or window incomplete), 5 done but temporary unicd cleanup failed.

Settings live in config.toml in the state directory; saving it applies it at once.
Set UNIC_HOME to run a separate profile, UNIC_RENDEZVOUS_URL to use another
pairing code service.
`)
	return b.String()
}
