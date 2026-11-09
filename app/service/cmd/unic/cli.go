package main

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/BurntSushi/toml"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func client() (*api.Client, error) {
	sock, err := paths.Socket()
	if err != nil {
		return nil, err
	}
	return api.NewClient(sock), nil
}

func runCLI(cmd string, args []string) error {
	c, err := client()
	if err != nil {
		return err
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	short, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()

	switch cmd {
	case "submit":
		return runSubmit(short, c, args)
	case "submission":
		if len(args) != 1 {
			return fmt.Errorf("usage: unic submission REQUEST_ID")
		}
		out, e := c.Submission(short, args[0])
		if e != nil {
			return e
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	case "received":
		return runReceived(short, c, args)
	case "capabilities":
		out, e := c.Capabilities(short)
		if e != nil {
			return e
		}
		return json.NewEncoder(os.Stdout).Encode(out)
	case "logs":
		return runLogLevels(short, c, args)
	case "diagnostics":
		if len(args) != 1 || args[0] != "telemetry" {
			return fmt.Errorf("usage: unic diagnostics telemetry")
		}
		state, err := c.Telemetry(short)
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(state)
	case "status":
		st, err := c.Status(short)
		if err != nil {
			return err
		}
		fmt.Printf("device  %s (%s)\n", st.Name, st.ID)
		printPeers(st.Peers)
		printPending(st.Pending)

	case "addr":
		ticket, err := c.ExportTicket(short)
		if err != nil {
			return err
		}
		fmt.Println(ticket)

	case "cache":
		if len(args) > 1 || (len(args) == 1 && args[0] != "--json") {
			return fmt.Errorf("usage: unic cache [--json]")
		}
		state, err := c.FileCache(short)
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(state)
	case "config":
		if len(args) == 1 && args[0] == "path" {
			p, err := paths.Config()
			if err != nil {
				return err
			}
			fmt.Println(p)
			return nil
		}
		if len(args) != 0 && (len(args) != 1 || args[0] != "--json") && (len(args) != 3 || args[0] != "set") && (len(args) != 2 || args[0] != "apply") {
			return fmt.Errorf("usage: unic config [path|--json|set <key> <TOML-value>|apply <file>]")
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
		if len(args) == 1 && args[0] == "--json" {
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
			fmt.Printf("# restart unicd to apply: %s\n", strings.Join(st.RestartRequired, ", "))
		}
		return toml.NewEncoder(os.Stdout).Encode(st.Config)

	case "peers":
		st, err := c.Status(short)
		if err != nil {
			return err
		}
		printPeers(st.Peers)

	case "pair":
		if len(args) != 1 {
			return fmt.Errorf("usage: unic pair <ticket>")
		}
		p, err := c.Pair(short, strings.TrimSpace(args[0]))
		if err != nil {
			return err
		}
		if p.Awaiting {
			me := "<this device's id>"
			if st, err := c.Status(short); err == nil {
				me = st.ID
			}
			fmt.Printf("added %s (%s); it will connect once %s approves this device\n  (on that device: `unic approve %s '<fingerprint from this device>'`)\nFingerprint: %s  (it must match the one shown there)\n", p.Name, p.ID, p.Name, me, p.Fingerprint)
		} else {
			fmt.Printf("paired with %s (%s)\n", p.Name, p.ID)
		}

	case "code":
		code, err := c.NewCode(short)
		if err != nil {
			return err
		}
		fmt.Printf("%s\n\nOn the other device: `unic join %s` (or 加入其他设备 in the window).\n", code.Display(), strings.ReplaceAll(code.Code, "-", ""))
		fmt.Printf("Valid until %s, for one device. It then shows up in `unic requests`;\ncompare the fingerprints on both devices before you approve it.\n", code.Expires().Format("15:04:05"))

	case "join":
		if len(args) == 0 {
			return fmt.Errorf("usage: unic join <6-digit code>")
		}
		p, err := c.Join(short, strings.Join(args, ""))
		if err != nil {
			return err
		}
		if p.Awaiting {
			fmt.Printf("found %s; it connects once it approves this device.\nFingerprint: %s  (it must match the one shown there)\n", p.Name, p.Fingerprint)
		} else {
			fmt.Printf("paired with %s (%s)\n", p.Name, p.ID)
		}

	case "requests":
		ps, err := c.Pending(short)
		if err != nil {
			return err
		}
		printPending(ps)

	case "approve", "reject":
		if (cmd == "approve" && len(args) != 2) || (cmd == "reject" && len(args) != 1) {
			return fmt.Errorf("usage: unic approve <id> '<fingerprint from the other device>'; unic reject <id>")
		}
		if cmd == "approve" {
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

	case "suggestions":
		cs, err := c.Candidates(short)
		if err != nil {
			return err
		}
		if len(cs) == 0 {
			fmt.Println("no suggested devices")
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

	case "suggest-pair", "suggest-ignore", "suggest-restore":
		if len(args) < 1 || (cmd == "suggest-pair" && len(args) != 2) {
			return fmt.Errorf("usage: unic suggest-pair <id> <backend-id> | unic %s <id>", cmd)
		}
		switch cmd {
		case "suggest-pair":
			p, err := c.PairCandidate(short, args[0], args[1])
			if err != nil {
				return err
			}
			fmt.Printf("requested %s; it connects once both sides approve.\nFingerprint: %s  (compare it on both devices)\n", p.Name, p.Fingerprint)
		default:
			if err := c.IgnoreCandidate(short, args[0], cmd == "suggest-ignore"); err != nil {
				return err
			}
			fmt.Println("ok")
		}

	case "unpair":
		if len(args) != 1 {
			return fmt.Errorf("usage: unic unpair <id>")
		}
		if err := c.Unpair(short, args[0]); err != nil {
			return err
		}
		fmt.Println("removed")

	case "copy":
		return copyCmd(short, c, args)

	case "paste":
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

	case "watch":
		err := c.Events(ctx, func(ev api.Event) {
			t := time.Unix(ev.Time, 0).Format("15:04:05")
			fmt.Printf("%s %-9s %s %s\n", t, ev.Kind, ev.Peer, ev.Info)
		})
		return err

	default:
		return fmt.Errorf("unknown command %q (try `unic help`)", cmd)
	}
	return nil
}

func copyCmd(ctx context.Context, c *api.Client, args []string) error {
	if len(args) > 0 && (args[0] == "-f" || args[0] == "--file") {
		if len(args) == 1 {
			return fmt.Errorf("usage: unic copy -f <file> [more files]")
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
		fmt.Println("no paired devices (run `unic pair <ticket>` with the other device's `unic addr`)")
		return
	}
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	_, _ = fmt.Fprintln(w, "ID\tNAME\tSTATE\tLAST SEEN")
	for _, p := range ps {
		state, seen := "offline", "never"
		if p.Connected {
			state = "online"
		}
		if p.LastSeen > 0 {
			seen = time.Since(time.Unix(p.LastSeen, 0)).Round(time.Second).String() + " ago"
		}
		_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", p.ID, p.Name, state, seen)
	}
	_ = w.Flush()
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
	fmt.Println("Approve with `unic approve <id> '<fingerprint from the other device>'`, dismiss with `unic reject <id>`.")
}
