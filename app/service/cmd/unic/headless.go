package main

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/clip"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/webtransfer"
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
	to := fs.String("to", "", "all, or a paired device ID or name; omit to start a temporary transfer")
	// Reuse the existing CLI flag parser for interspersed flags.
	pos, err := parse(fs, args)
	if err != nil {
		return err
	}
	if *to == "" && *request != "" {
		return fmt.Errorf("--request-id applies only with --to")
	}
	if *request == "" {
		*request = rand.Text()
	}
	hasDirectory := false
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
			if !st.Mode().IsRegular() && !st.IsDir() {
				return fmt.Errorf("send requires regular files or directories")
			}
			hasDirectory = hasDirectory || st.IsDir()
			size := st.Size()
			if st.IsDir() {
				size = 0
			}
			s.Files = append(s.Files, clip.File{Name: st.Name(), Path: abs, Size: size, Directory: st.IsDir()})
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
	if *to == "" && hasDirectory {
		return fmt.Errorf("directory send requires --to all or a paired device")
	}
	if *to == "" {
		return runTemporaryTransfer(ctx, c, s, *asJSON)
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
	out, err := c.Submit(ctx, api.SubmitInput{Async: hasDirectory, RequestID: *request, Snapshot: s, DeviceIDs: devices})
	if err != nil {
		return err
	}
	if out.Phase == engine.SubmissionProcessing {
		out, err = c.WaitSubmission(ctx, *request, func(stage string) {
			if stage != "" {
				fmt.Fprintln(os.Stderr, stage)
			}
		})
		if err != nil {
			return err
		}
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

// runReceive is `unic recv`. A first word that is not a list command is a source:
// a 6-digit code (spaces and dashes allowed) or a transfer link.
func runReceive(ctx context.Context, c *api.Client, args []string) error {
	if len(args) == 0 {
		return runReceived(ctx, c, args)
	}
	switch args[0] {
	case "list", "show", "dismiss":
		return runReceived(ctx, c, args)
	}
	asJSON := false
	if i := slices.Index(args, "--json"); i >= 0 {
		asJSON = true
		args = slices.Delete(slices.Clone(args), i, i+1)
	}
	source := strings.Join(args, "")
	// Reject a source that cannot be a code or link before any request acquires a service.
	if _, msg := webtransfer.SourceProblem(source); msg != "" {
		return errors.New(msg)
	}
	out, err := c.Receive(ctx, api.ReceiveInput{Source: source})
	if err != nil {
		return err
	}
	if asJSON {
		return json.NewEncoder(os.Stdout).Encode(out)
	}
	for _, it := range out.Items {
		if it.Kind == "text" {
			fmt.Println(it.Text)
			continue
		}
		fmt.Fprintf(os.Stderr, "received %s %s (%d bytes)\n", it.Kind, it.Name, it.Size)
	}
	fmt.Fprintln(os.Stderr, "stored in history; see `unic history list` and `unic recv list`")
	return nil
}

// runTemporaryTransfer offers the content to one receiver that has the code or the
// link, and waits until it is delivered, fails or expires.
func runTemporaryTransfer(ctx context.Context, c *api.Client, s clip.Snapshot, asJSON bool) error {
	t, err := c.StartTransfer(ctx, api.StartTransferInput{Snapshot: s})
	if err != nil {
		return err
	}
	wait := &spinner{stop: make(chan struct{})}
	if asJSON {
		if err := json.NewEncoder(os.Stdout).Encode(t); err != nil { // the first line carries the link and the code
			return err
		}
	} else {
		if t.Code != "" {
			fmt.Printf("Code: %s\n", style(outTTY, sBold, strings.Replace(t.Code, "-", " ", 1)))
		}
		fmt.Printf("Link: %s\n", t.Link)
		note("On the other device run: unic recv <code or link>")
		wait = startSpinner(fmt.Sprintf("Waiting for a receiver until %s (Ctrl+C cancels)", time.UnixMilli(t.ExpiresAtMs).Format("15:04:05")))
	}
	defer wait.Stop()
	for {
		select {
		case <-ctx.Done():
			cctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			_ = c.CancelTransfer(cctx, t.ID)
			cancel()
			return &incompleteError{"cancelled; nothing was delivered"}
		case <-time.After(500 * time.Millisecond):
		}
		cur, err := c.Transfer(ctx, t.ID)
		if err != nil {
			if ctx.Err() != nil {
				continue
			}
			return err
		}
		switch cur.State {
		case api.TransferWaiting, api.TransferSending:
			continue
		case api.TransferDelivered:
			if asJSON {
				return json.NewEncoder(os.Stdout).Encode(cur)
			}
			wait.Stop()
			fmt.Fprintln(os.Stderr, style(errTTY, sOK, "✓")+" delivered")
			return nil
		}
		if asJSON {
			_ = json.NewEncoder(os.Stdout).Encode(cur)
		}
		reason := cur.State
		if cur.Reason != "" {
			reason += ": " + cur.Reason
		}
		return &incompleteError{"not delivered (" + reason + ")"}
	}
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
