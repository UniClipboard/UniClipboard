package main

import (
	"context"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"text/tabwriter"
	"time"

	"unic/internal/api"
	"unic/internal/engine"
	"unic/internal/paths"
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
	case "status":
		st, err := c.Status(short)
		if err != nil {
			return err
		}
		fmt.Printf("device  %s (%s)\n", st.Name, st.ID)
		if st.Addr == "" {
			fmt.Println("address (starting…)")
		} else {
			fmt.Printf("address %s…  (run `unic addr` for the full address)\n", trunc(st.Addr, 24))
		}
		printPeers(st.Peers)
		printPending(st.Pending)

	case "addr":
		st, err := c.Status(short)
		if err != nil {
			return err
		}
		if st.Addr == "" {
			return fmt.Errorf("the daemon is still starting; try again in a moment")
		}
		fmt.Println(st.Addr)

	case "peers":
		st, err := c.Status(short)
		if err != nil {
			return err
		}
		printPeers(st.Peers)

	case "pair":
		if len(args) != 1 {
			return fmt.Errorf("usage: unic pair <address>")
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
			fmt.Printf("added %s (%s); it will connect once %s approves this device\n  (on that device: `unic approve %s`)\nFingerprint: %s  (it must match the one shown there)\n", p.Name, p.ID, p.Name, me, p.Fingerprint)
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
		if len(args) != 1 {
			return fmt.Errorf("usage: unic %s <id>", cmd)
		}
		if cmd == "approve" {
			p, err := c.Approve(short, args[0])
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
		err := c.Events(ctx, func(ev engine.Event) {
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
			return fmt.Errorf("usage: unic copy -f <file>...")
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

func printPeers(ps []engine.PeerStatus) {
	if len(ps) == 0 {
		fmt.Println("no paired devices (run `unic pair <address>` with the other device's `unic addr`)")
		return
	}
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "ID\tNAME\tSTATE\tLAST SEEN")
	for _, p := range ps {
		state, seen := "offline", "never"
		if p.Connected {
			state = "online"
		}
		if p.LastSeen > 0 {
			seen = time.Since(time.Unix(p.LastSeen, 0)).Round(time.Second).String() + " ago"
		}
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", p.ID, p.Name, state, seen)
	}
	w.Flush()
}

func printPending(ps []engine.PendingPeer) {
	if len(ps) == 0 {
		return
	}
	fmt.Println("\nWaiting for your approval:")
	w := tabwriter.NewWriter(os.Stdout, 0, 0, 2, ' ', 0)
	fmt.Fprintln(w, "ID\tNAME\tFROM\tFINGERPRINT")
	for _, p := range ps {
		from := "connected directly"
		switch {
		case p.ByCode:
			from = "used the pairing code"
		case p.Via != "":
			from = "introduced by " + p.Via
		}
		fmt.Fprintf(w, "%s\t%s\t%s\t%s\n", p.ID, p.Name, from, p.Fingerprint)
	}
	w.Flush()
	fmt.Println("Approve with `unic approve <id>`, dismiss with `unic reject <id>`.")
}

func trunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
