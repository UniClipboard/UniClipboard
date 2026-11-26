package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func runCLI(ctx context.Context, c *api.Client, l leaf, args []string) error {
	// Request deadlines come from the client transport and start after service acquisition.
	short := ctx

	switch l.Name {
	case "send":
		return runSubmit(short, c, args)
	case "dev submission":
		if len(args) != 1 {
			return fmt.Errorf("usage: unic dev submission REQUEST_ID")
		}
		out, e := c.Submission(short, args[0])
		if e != nil {
			return e
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	case "recv":
		return runReceive(ctx, c, args)
	case "dev capabilities":
		out, e := c.Capabilities(short)
		if e != nil {
			return e
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	case "dev logs level":
		return runLogLevels(short, c, append([]string{"level"}, args...))
	case "dev logs export":
		return runLogExport(short, args)
	case "dev logs help":
		fmt.Println("usage: unic dev logs level [debug|info|warn|error|reset] [--target local|remote] [--for 15m]\n" + logExportUsage)
		return nil
	case "dev diagnostics telemetry":
		if len(args) != 0 {
			return fmt.Errorf("usage: unic dev diagnostics telemetry")
		}
		state, err := c.Telemetry(short)
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(state)
	case "status":
		return runStatus(ctx, args, globalJSON)

	case "channel pair":
		return runChannelPair(short, c, args)
	case "channel add":
		return runChannelAdd(short, c, args)
	case "channel pause", "channel resume", "channel remove":
		action := strings.TrimPrefix(l.Name, "channel ")
		return runChannelManage(short, c, append([]string{action}, args...))

	case "device addr":
		backend := ""
		if len(args) != 0 {
			if len(args) != 2 || args[0] != "--backend" || args[1] == "" {
				return usageError("usage: unic device addr [--backend INSTANCE]")
			}
			backend = args[1]
		}
		ticket, err := c.ExportTicketVia(short, backend)
		if err != nil {
			return err
		}
		fmt.Println(ticket)
		if current.temporary() {
			fmt.Fprintln(os.Stderr, "this address works only while a unic service runs; run `unic daemon start` to keep it reachable.")
		}

	case "dev cache":
		if len(args) > 1 || (len(args) == 1 && args[0] != "--json") {
			return usageError("usage: unic dev cache [--json]")
		}
		state, err := c.FileCache(short)
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(state)
	case "config path":
		if len(args) != 0 {
			return usageError("usage: unic config path")
		}
		p, err := paths.ResolveConfig()
		if err != nil {
			return err
		}
		fmt.Println(p)
		return nil
	case "config":
		if len(args) != 0 && (len(args) != 1 || args[0] != "--json") && (len(args) != 3 || args[0] != "set") && (len(args) != 2 || args[0] != "apply") {
			return usageError("usage: unic config [--json|set <key> <TOML-value>|apply <file>] | unic config path")
		}
		st, err := c.Config(short)
		if err != nil {
			return err
		}
		if len(args) > 1 {
			var raw []byte
			if args[0] == "set" {
				raw, err = config.Set(st.Config, args[1], args[2])
			} else {
				raw, err = os.ReadFile(args[1])
			}
			if err != nil {
				return err
			}
			st, err = c.SaveConfig(short, st.Revision, raw)
			if err != nil {
				return err
			}
		}
		if globalJSON || (len(args) == 1 && args[0] == "--json") {
			return json.NewEncoder(os.Stdout).Encode(st)
		}
		if st.ApplyError != "" {
			return fmt.Errorf("%s", st.ApplyError)
		}
		fmt.Printf("# file: %s  (edit and save; it applies at once)\n", st.Path)
		if st.Error != "" {
			fmt.Printf("# ERROR, last edit not applied (the settings below are the previous ones):\n#   %s\n", st.Error)
		}
		if len(st.RestartRequired) > 0 {
			fmt.Printf("# run `unic daemon restart` to apply: %s\n", strings.Join(st.RestartRequired, ", "))
		}
		return toml.NewEncoder(os.Stdout).Encode(st.Config)

	case "device path":
		if len(args) < 3 {
			return errors.New("usage: unic device path pause|resume|remove|policy BINDING_ID REVISION [--text DIR] [--images DIR] [--files DIR]")
		}
		revision, err := strconv.ParseInt(args[2], 10, 64)
		if err != nil || revision <= 0 {
			return errors.New("positive binding revision required")
		}
		switch args[0] {
		case "pause", "resume":
			if len(args) != 3 {
				return errors.New("usage: unic device path pause|resume|remove BINDING_ID REVISION")
			}
			en := args[0] == "resume"
			out, err := c.SetBindingSync(short, args[1], api.BindingEdit{Revision: revision, Enabled: &en})
			if err != nil {
				return err
			}
			return json.NewEncoder(os.Stdout).Encode(out)
		case "remove":
			if len(args) != 3 {
				return errors.New("usage: unic device path pause|resume|remove BINDING_ID REVISION")
			}
			return c.RemoveBinding(short, args[1], revision)
		case "policy":
			if len(args) == 3 {
				return errors.New("usage: unic device path policy BINDING_ID REVISION [--text DIR] [--images DIR] [--files DIR]")
			}
			pol := engine.DefaultBindingSyncPolicy()
			if st, serr := c.Status(short); serr == nil {
				for _, p := range st.Peers {
					for _, b := range p.Bindings {
						if b.ID == args[1] {
							pol = b.Policy.Normalized()
						}
					}
				}
			}
			for i := 3; i < len(args); i += 2 {
				if i+1 >= len(args) {
					return errors.New("missing direction value (bidirectional|send_only|receive_only|disabled)")
				}
				dir := engine.SyncDirection(args[i+1])
				if !dir.Valid() {
					return fmt.Errorf("invalid sync direction %q (want bidirectional|send_only|receive_only|disabled)", args[i+1])
				}
				switch args[i] {
				case "--text":
					pol.Text = dir
				case "--images":
					pol.Images = dir
				case "--files":
					pol.Files = dir
				default:
					return fmt.Errorf("unknown policy flag %q", args[i])
				}
			}
			out, err := c.SetBindingSync(short, args[1], api.BindingEdit{Revision: revision, Policy: &pol})
			if err != nil {
				return err
			}
			return json.NewEncoder(os.Stdout).Encode(out)
		}
		return errors.New("unknown binding action")
	case "device backends":
		asJSON := globalJSON
		for _, a := range args {
			if a == "--json" {
				asJSON = true
			} else {
				return usageError("usage: unic device backends [--json]")
			}
		}
		backends, err := c.SyncBackends(short)
		if err != nil {
			return err
		}
		if asJSON || !outTTY {
			return json.NewEncoder(os.Stdout).Encode(backends)
		}
		if len(backends) == 0 {
			fmt.Println("no sync backends configured")
			return nil
		}
		w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
		_, _ = fmt.Fprintln(w, "ID\tIMPLEMENTATION\tTRANSPORT\tPAIRING\tSTATE")
		for _, b := range backends {
			_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%t\t%s\n", b.ID, b.ImplementationID, b.TransportKind, b.Pairing, b.State)
		}
		return w.Flush()

	case "device list":
		asJSON := globalJSON
		var refresh bool
		for _, a := range args {
			switch a {
			case "--json":
				asJSON = true
			case "--refresh":
				refresh = true
			default:
				return usageError("usage: unic device list [--refresh] [--json]")
			}
		}
		var st api.Status
		var err error
		if refresh {
			rctx, rcancel := context.WithTimeout(ctx, 12*time.Second)
			st, err = c.RefreshDevices(rctx, "")
			rcancel()
		} else {
			st, err = c.Status(short)
		}
		if err != nil {
			return err
		}
		if asJSON {
			return json.NewEncoder(os.Stdout).Encode(st)
		}
		printPeers(st.Peers)
		printPending(st.Pending)

	case "device rename":
		return runDeviceRename(short, c, args)

	case "device add":
		backend := ""
		var positional []string
		for i := 0; i < len(args); i++ {
			if args[i] == "--backend" {
				i++
				if i >= len(args) || args[i] == "" || backend != "" {
					return errors.New("usage: unic device add [CODE|TICKET] [--backend INSTANCE]")
				}
				backend = args[i]
			} else {
				positional = append(positional, args[i])
			}
		}
		args = positional
		switch digits := strings.ReplaceAll(strings.ReplaceAll(strings.Join(args, ""), "-", ""), " ", ""); {
		case len(args) == 0:
			code, err := c.NewCodeVia(short, backend)
			if err != nil {
				return err
			}
			fmt.Printf("%s\n\nOn the other device: `unic device add %s` (or 加入其他设备 in the window).\n", code.Display(), strings.ReplaceAll(code.Code, "-", ""))
			fmt.Printf("Valid until %s, for one device. It then shows up in `unic device list`;\ncompare the fingerprints on both devices before you approve it.\n", code.Expires().Format("15:04:05"))
			return holdInvitation(ctx, c, code)

		case isSixDigits(digits):
			p, err := c.JoinVia(short, digits, backend)
			if err != nil {
				return err
			}
			if p.Awaiting {
				fmt.Printf("found %s; it connects once it approves this device.\nFingerprint: %s  (it must match the one shown there)\n", p.Name, p.Fingerprint)
			} else {
				fmt.Printf("paired with %s (%s)\n", p.Name, p.ID)
			}
			return awaitPairing(ctx, c, p)

		case len(args) == 1:
			p, err := c.PairVia(short, strings.TrimSpace(args[0]), backend)
			if err != nil {
				return err
			}
			if p.Awaiting {
				me := "<this device's id>"
				if st, err := c.Status(short); err == nil {
					me = st.ID
				}
				fmt.Printf("added %s (%s); it will connect once %s approves this device\n  (on that device: `unic device approve %s '<fingerprint from this device>'`)\nFingerprint: %s  (it must match the one shown there)\n", p.Name, p.ID, p.Name, me, p.Fingerprint)
			} else {
				fmt.Printf("paired with %s (%s)\n", p.Name, p.ID)
			}
			return awaitPairing(ctx, c, p)

		default:
			return fmt.Errorf("usage: unic device add [6-digit code | address]")
		}

	case "device approve", "device reject":
		approve := l.Name == "device approve"
		if (approve && len(args) != 2) || (!approve && len(args) != 1) {
			return fmt.Errorf("usage: unic device approve <id> '<fingerprint from the other device>'; unic device reject <id>")
		}
		if approve {
			ps, err := c.Pending(short)
			if err != nil {
				return err
			}
			var approval api.Approval
			for _, p := range ps {
				if p.ID == args[0] {
					approval = api.Approval{Fingerprint: args[1], Revision: p.Revision, Bindings: p.Bindings}
					break
				}
			}
			if approval.Revision == 0 {
				return fmt.Errorf("no pending request for this device")
			}
			p, err := c.Approve(short, args[0], approval)
			if err != nil {
				return err
			}
			fmt.Printf("approved %s (%s); connecting…\n", p.Name, p.ID)
		} else {
			if err := c.Reject(short, args[0]); err != nil {
				return err
			}
			fmt.Println("rejected")
		}

	case "device suggest":
		return runDeviceSuggest(short, c, args)

	case "device suggestions":
		return runDeviceSuggest(short, c, append([]string{"list"}, args...))

	case "device suggest-pair":
		return runDeviceSuggest(short, c, append([]string{"pair"}, args...))

	case "device suggest-ignore":
		return runDeviceSuggest(short, c, append([]string{"ignore"}, args...))

	case "device suggest-restore":
		return runDeviceSuggest(short, c, append([]string{"restore"}, args...))

	case "device channel-add":
		return runChannelAdd(short, c, args)

	case "device channel":
		return runChannelManage(short, c, args)

	case "device remove":
		if len(args) != 1 {
			return usageError("usage: unic device remove <id>")
		}
		if err := c.Unpair(short, args[0]); err != nil {
			return err
		}
		fmt.Println("removed")

	case "dev copy":
		return copyCmd(short, c, args)

	case "dev paste":
		v, err := c.Clipboard(short)
		if err != nil {
			return err
		}
		switch {
		case len(v.Files) > 0:
			fmt.Println(strings.Join(v.Files, "\n"))
		case v.Text != "":
			fmt.Print(v.Text)
		default:
			return fmt.Errorf("the clipboard holds no text or files (%s)", v.Summary)
		}

	case "dev watch":
		err := c.Events(ctx, func(ev api.Event) {
			t := time.Unix(ev.Time, 0).Format("15:04:05")
			fmt.Printf("%s %-9s %s %s\n", t, ev.Kind, ev.Peer, ev.Info)
		})
		var acquire *acquireError
		if ctx.Err() == nil && !errors.As(err, &acquire) && !errors.Is(err, api.ErrNoDaemon) {
			// The stream ended without a cancel: the service stopped or was replaced.
			return &incompleteError{"the service stopped or was replaced; the event stream ended"}
		}
		return err

	default:
		return fmt.Errorf("unknown command %q (try `unic help`)", l.Name)
	}
	return nil
}

func copyCmd(ctx context.Context, c *api.Client, args []string) error {
	if len(args) > 0 && (args[0] == "-f" || args[0] == "--file") {
		if len(args) == 1 {
			return fmt.Errorf("usage: unic dev copy -f <file> [more files]")
		}
		var files []string
		for _, a := range args[1:] {
			abs, err := filepath.Abs(a)
			if err != nil {
				return err
			}
			files = append(files, abs)
		}
		return c.SetClipboard(ctx, api.SetClipboard{Files: files})
	}
	var text string
	if len(args) > 0 {
		text = strings.Join(args, " ")
	} else {
		b, err := io.ReadAll(io.LimitReader(os.Stdin, 32<<20))
		if err != nil {
			return err
		}
		text = string(b)
	}
	return c.SetClipboard(ctx, api.SetClipboard{Text: &text})
}

func printPeers(ps []api.PeerStatus) {
	if len(ps) == 0 {
		fmt.Println("no paired devices (run `unic device add <address>` with the other device's `unic device addr`)")
		return
	}
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	_, _ = fmt.Fprintln(w, "ID\tNAME\tSTATE\tLAST SEEN")
	for _, p := range ps {
		state, seen := peerStateSummary(p), "never"
		if p.LastSeen > 0 {
			seen = time.Since(time.Unix(p.LastSeen, 0)).Round(time.Second).String() + " ago"
		}
		_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", p.ID, p.Name, state, seen)
	}
	_ = w.Flush()
}

func peerStateSummary(p api.PeerStatus) string {
	if p.Kind == "external" {
		if p.Enabled {
			return "enabled"
		}
		return "disabled"
	}
	var connected, connecting bool
	var path string
	var latencyMs int64
	for _, b := range p.Bindings {
		if b.State != engine.BindingAuthorized {
			continue
		}
		switch b.Connection {
		case engine.Connected:
			connected = true
			if path == "" && b.ConnectionPath != "" && b.ConnectionPath != "unknown" {
				path = b.ConnectionPath
			}
			if latencyMs == 0 && b.LatencyMs > 0 {
				latencyMs = b.LatencyMs
			}
		case engine.Connecting:
			connecting = true
		}
	}
	switch {
	case connected || p.Connected:
		var detail []string
		switch path {
		case "direct":
			detail = append(detail, "direct")
		case "derp":
			detail = append(detail, "relay")
		case "peer_relay":
			detail = append(detail, "peer-relay")
		}
		if latencyMs > 0 {
			detail = append(detail, fmt.Sprintf("%dms", latencyMs))
		}
		if len(detail) > 0 {
			return "online (" + strings.Join(detail, ", ") + ")"
		}
		return "online"
	case connecting:
		return "reconnecting"
	default:
		return "offline"
	}
}

func printPending(ps []api.PendingPeer) {
	if len(ps) == 0 {
		return
	}
	fmt.Println("\nWaiting for your approval:")
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	_, _ = fmt.Fprintln(w, "ID\tNAME\tFROM\tFINGERPRINT")
	for _, p := range ps {
		from := "connected directly"
		switch {
		case p.ByCode:
			from = "used the pairing code"
		case p.Via != "":
			from = "introduced by " + p.Via
		}
		_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", p.ID, p.Name, from, p.Fingerprint)
	}
	_ = w.Flush()
	fmt.Println("Approve with `unic device approve <id> '<fingerprint from the other device>'`, dismiss with `unic device reject <id>`.")
}

func isSixDigits(s string) bool {
	if len(s) != 6 {
		return false
	}
	for _, r := range s {
		if r < '0' || r > '9' {
			return false
		}
	}
	return true
}

func runStatus(ctx context.Context, args []string, asJSONDefault bool) error {
	fs := flag.NewFlagSet("status", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", asJSONDefault, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 0 {
		return usageError("usage: unic status [--json]")
	}
	dir, err := paths.CanonicalDir()
	if err != nil {
		return err
	}
	socket, err := paths.ResolveSocket()
	if err != nil {
		return err
	}
	profileName, _ := paths.ActiveProfile()
	raw := api.NewClient(socket)
	pctx, pcancel := context.WithTimeout(ctx, probeTimeout)
	inst, serr := raw.Service(pctx)
	pcancel()

	if errors.Is(serr, api.ErrNoDaemon) {
		if *asJSON {
			return json.NewEncoder(os.Stdout).Encode(map[string]any{
				"profile":   profileName,
				"directory": dir,
				"daemon":    map[string]any{"running": false},
			})
		}
		fmt.Printf("profile   %s (%s)\n", profileName, dir)
		fmt.Printf("daemon    stopped\n")
		note("run `unic daemon start` for continuous background sync")
		return nil
	}
	if serr != nil {
		return fmt.Errorf("inspect existing unicd: %w", serr)
	}
	if inst.Directory != dir {
		return errors.New("socket belongs to a different state directory")
	}

	qctx, qcancel := context.WithTimeout(ctx, 5*time.Second)
	st, err := raw.Status(qctx)
	qcancel()
	if err != nil {
		return err
	}
	var caps *capabilitiesSummary
	cctx, ccancel := context.WithTimeout(ctx, probeTimeout)
	if c, cerr := raw.Capabilities(cctx); cerr == nil {
		v := capabilitiesView(c)
		caps = &v
	}
	ccancel()

	if *asJSON {
		payload := map[string]any{
			"profile":   profileName,
			"directory": dir,
			"daemon": map[string]any{
				"running":       true,
				"mode":          inst.Mode,
				"pid":           inst.PID,
				"started_at_ms": inst.StartedAtMs,
				"leases":        inst.Leases,
			},
			"id":      st.ID,
			"name":    st.Name,
			"peers":   st.Peers,
			"pending": st.Pending,
		}
		if caps != nil {
			payload["clipboard"] = caps
		}
		return json.NewEncoder(os.Stdout).Encode(payload)
	}

	uptime := formatDurationShort(time.Since(time.UnixMilli(inst.StartedAtMs)))
	fmt.Printf("profile   %s (%s)\n", profileName, dir)
	fmt.Printf("daemon    running (%s, pid %d, up %s)\n", inst.Mode, inst.PID, uptime)
	fmt.Printf("device    %s (%s)\n", st.Name, st.ID)
	if caps != nil {
		fmt.Printf("clipboard %s\n", caps.Summary)
	}
	fmt.Println()
	printPeers(st.Peers)
	printPending(st.Pending)
	return nil
}

func runDeviceRename(ctx context.Context, c *api.Client, args []string) error {
	fs := flag.NewFlagSet("device rename", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	clearName := fs.Bool("clear", false, "clear custom name and revert to reported name")
	asJSON := fs.Bool("json", globalJSON, "output JSON")
	pos, err := parse(fs, args)
	if err != nil {
		return usageError("usage: unic device rename <id> [NAME] [--clear] [--json]")
	}
	if (*clearName && len(pos) != 1) || (!*clearName && len(pos) < 2) {
		return usageError("usage: unic device rename <id> [NAME] [--clear] [--json]")
	}
	id, err := resolveDevice(ctx, c, pos[0])
	if err != nil {
		return err
	}
	st, err := c.Status(ctx)
	if err != nil {
		return err
	}
	var rev int64
	for _, p := range st.Peers {
		if p.ID == id {
			rev = p.Revision
			break
		}
	}
	if rev <= 0 {
		return fmt.Errorf("device %q has no valid revision", id)
	}
	newName := ""
	if !*clearName {
		newName = strings.Join(pos[1:], " ")
	}
	updated, err := c.RenamePeer(ctx, id, api.PeerRename{Revision: rev, Name: newName})
	if err != nil {
		return err
	}
	if *asJSON {
		return json.NewEncoder(os.Stdout).Encode(updated)
	}
	fmt.Printf("renamed %s -> %s\n", updated.ID, updated.Name)
	return nil
}

func runDeviceSuggest(ctx context.Context, c *api.Client, args []string) error {
	sub := "list"
	rest := args
	if len(args) > 0 && !strings.HasPrefix(args[0], "-") {
		sub = args[0]
		rest = args[1:]
	}
	switch sub {
	case "list", "ls":
		asJSON := globalJSON
		for _, a := range rest {
			if a == "--json" {
				asJSON = true
			} else {
				return usageError("usage: unic device suggest [list] [--json]")
			}
		}
		cs, err := c.Candidates(ctx)
		if err != nil {
			return err
		}
		if asJSON {
			return json.NewEncoder(os.Stdout).Encode(cs)
		}
		if len(cs) == 0 {
			fmt.Println("no suggested devices")
			return nil
		}
		for _, d := range cs {
			state := ""
			switch {
			case d.Ignored:
				state = "  [ignored]"
			case d.Requested:
				state = "  [pairing requested]"
			}
			fmt.Printf("%s  %s  via %s (%s)%s\n", d.ID, d.Name, strings.Join(d.IntroducedBy, ", "), d.BackendID, state)
		}
		return nil
	case "pair":
		if len(rest) != 2 {
			return usageError("usage: unic device suggest pair <id> <backend-id>")
		}
		p, err := c.PairCandidate(ctx, rest[0], rest[1])
		if err != nil {
			return err
		}
		fmt.Printf("requested %s; it connects once both sides approve.\nFingerprint: %s  (compare it on both devices)\n", p.Name, p.Fingerprint)
		return nil
	case "ignore", "restore":
		if len(rest) != 1 {
			return usageError(fmt.Sprintf("usage: unic device suggest %s <id>", sub))
		}
		if err := c.IgnoreCandidate(ctx, rest[0], sub == "ignore"); err != nil {
			return err
		}
		fmt.Println("ok")
		return nil
	default:
		return usageError("usage: unic device suggest [list | pair <id> <backend-id> | ignore <id> | restore <id>]")
	}
}

func runChannelAdd(ctx context.Context, c *api.Client, args []string) error {
	fs := flag.NewFlagSet("channel add", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", globalJSON, "output JSON")
	pos, err := parse(fs, args)
	if err != nil || len(pos) != 2 {
		return usageError("usage: unic channel add <instance> <name> [--json]")
	}
	channel, err := c.AddChannel(ctx, api.ChannelAdd{BackendID: pos[0], Name: pos[1]})
	if err != nil {
		return err
	}
	if *asJSON || !outTTY {
		return json.NewEncoder(os.Stdout).Encode(channel)
	}
	fmt.Printf("%s added channel %s (%s)\n", style(outTTY, sOK, "✓"), channel.Name, channel.ID)
	return nil
}

func runChannelManage(ctx context.Context, c *api.Client, args []string) error {
	if len(args) < 1 {
		return usageError("usage: unic channel pause|resume|remove <id> <revision> [--json]")
	}
	action := args[0]
	fs := flag.NewFlagSet("channel "+action, flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", globalJSON, "output JSON")
	pos, err := parse(fs, args[1:])
	if err != nil || len(pos) != 2 {
		return usageError(fmt.Sprintf("usage: unic channel %s <id> <revision>", action))
	}
	revision, err := strconv.ParseInt(pos[1], 10, 64)
	if err != nil || revision <= 0 {
		return usageError("invalid channel revision")
	}
	switch action {
	case "pause", "resume":
		enabled := action == "resume"
		channel, err := c.UpdateChannel(ctx, pos[0], api.ChannelUpdate{Revision: revision, Enabled: &enabled})
		if err != nil {
			return err
		}
		if *asJSON || !outTTY {
			return json.NewEncoder(os.Stdout).Encode(channel)
		}
		fmt.Printf("%s channel %s %sd\n", style(outTTY, sOK, "✓"), channel.ID, action)
		return nil
	case "remove":
		if err := c.RemoveChannel(ctx, pos[0], revision); err != nil {
			return err
		}
		if *asJSON {
			return json.NewEncoder(os.Stdout).Encode(map[string]any{"removed": true, "id": pos[0]})
		}
		fmt.Println("removed")
		return nil
	default:
		return usageError("expected pause, resume, or remove")
	}
}

var _ = servicelifecycle.ModeStandalone
