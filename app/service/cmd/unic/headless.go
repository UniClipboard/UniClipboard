package main

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// resolveDevice maps a device ID or exact name to a paired device ID.
func resolveDevice(ctx context.Context, c *api.Client, ref string) (string, error) {
	st, err := c.Status(ctx)
	if err != nil {
		return "", err
	}
	var hit []string
	for _, p := range st.Peers {
		if p.ID == ref {
			return p.ID, nil
		}
		if p.Name == ref {
			hit = append(hit, p.ID)
		}
	}
	switch len(hit) {
	case 1:
		return hit[0], nil
	case 0:
		return "", fmt.Errorf("no paired device %q (see `unic device list`)", ref)
	}
	return "", fmt.Errorf("%d paired devices are named %q; use the ID from `unic device list`", len(hit), ref)
}

func runSubmit(ctx context.Context, c *api.Client, args []string) error {
	fs := flag.NewFlagSet("send", flag.ContinueOnError)
	request := fs.String("request-id", "", "idempotency key")
	files := fs.Bool("f", false, "submit files")
	png := fs.String("png", "", "submit a PNG representation")
	asJSON := fs.Bool("json", false, "print commit and network result")
	to := fs.String("to", "all", "all, or a paired device ID or name")
	// Reuse the existing CLI flag parser for interspersed flags.
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if *request == "" {
		*request = rand.Text()
	}
	var s clip.Snapshot
	switch {
	case *png != "":
		if *files || len(pos) != 0 {
			return fmt.Errorf("use either --png, -f, or text")
		}
		data, e := os.ReadFile(*png)
		if e != nil {
			return e
		}
		s.Parts = []clip.Part{{Format: clip.PNG, Data: data}}
	case *files:
		if len(pos) == 0 {
			return fmt.Errorf("send -f requires files")
		}
		for _, p := range pos {
			abs, e := filepath.Abs(p)
			if e != nil {
				return e
			}
			st, e := os.Stat(abs)
			if e != nil {
				return e
			}
			if !st.Mode().IsRegular() {
				return fmt.Errorf("send requires regular files")
			}
			s.Files = append(s.Files, clip.File{Name: st.Name(), Path: abs, Size: st.Size()})
		}
	default:
		text := strings.Join(pos, " ")
		if len(pos) == 0 {
			b, e := io.ReadAll(io.LimitReader(os.Stdin, (64<<20)+1))
			if e != nil {
				return e
			}
			if len(b) > 64<<20 {
				return fmt.Errorf("stdin exceeds 64 MiB")
			}
			text = string(b)
		}
		s.Parts = []clip.Part{{Format: clip.Text, Data: []byte(text)}}
	}
	// Print the key before the request so a caller can query an interrupted submission.
	fmt.Fprintln(os.Stderr, "request_id="+*request)
	var devices []string
	if *to != "all" {
		id, e := resolveDevice(ctx, c, *to)
		if e != nil {
			return e
		}
		devices = []string{id}
	}
	connectOpportunity(ctx, c)
	out, err := c.Submit(ctx, api.SubmitInput{RequestID: *request, Snapshot: s, DeviceIDs: devices})
	if err != nil {
		return err
	}
	// The submission is committed once. Only this command's own operation is observed.
	outcome, op, err := awaitSend(ctx, c, out.OperationID)
	if err != nil {
		return err
	}
	if *asJSON {
		if err := json.NewEncoder(os.Stdout).Encode(submitReport{Submission: out, Send: sendReport{Outcome: outcome, Operation: op}}); err != nil {
			return err
		}
	} else {
		fmt.Printf("request_id=%s history_id=%s phase=%s sync=%s send=%s\n", out.RequestID, out.HistoryID, out.Phase, out.SyncDisposition, outcome)
	}
	return sendIncomplete(outcome, *request)
}

// submitReport keeps local commit and network outcome as separate fields.
type submitReport struct {
	Submission engine.SubmissionResult `json:"submission"`
	Send       sendReport              `json:"send"`
}
type sendReport struct {
	Outcome   string            `json:"outcome"`
	Operation *engine.Operation `json:"operation,omitempty"`
}

func runReceived(ctx context.Context, c *api.Client, args []string) error {
	if len(args) == 0 || args[0] == "list" {
		v, e := c.Received(ctx)
		if e != nil {
			return e
		}
		return json.NewEncoder(os.Stdout).Encode(v)
	}
	switch args[0] {
	case "show":
		if len(args) < 2 || len(args) > 3 {
			return fmt.Errorf("usage: unic recv show ID [PART]")
		}
		part := 0
		if len(args) == 3 {
			v, e := strconv.Atoi(args[2])
			if e != nil || v < 0 {
				return fmt.Errorf("invalid part")
			}
			part = v
		}
		return c.CopyReceivedContent(ctx, args[1], part, os.Stdout)
	case "dismiss":
		if len(args) != 2 {
			return fmt.Errorf("usage: unic recv dismiss ID")
		}
		return c.DismissReceived(ctx, args[1])
	}
	return fmt.Errorf("usage: unic recv list|show|dismiss")
}
