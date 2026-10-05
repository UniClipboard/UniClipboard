package commands

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"golang.org/x/term"
)

// sendInputKind enumerates the resolved input of `send` in new-entry mode.
type sendInputKind int

const (
	sendInputStdin sendInputKind = iota
	sendInputStdinFiles
	sendInputText
	sendInputFile
	sendInputFiles
)

type sendInput struct {
	kind  sendInputKind
	text  string
	paths []string
}

// perTargetOutcome mirrors `PerTargetOutcomeDto`.
type perTargetOutcome struct {
	DeviceID string  `json:"deviceId"`
	Outcome  string  `json:"outcome"`
	Error    *string `json:"error,omitempty"`
}

// dispatchOutcome mirrors `DispatchOutcomeResponse`.
type dispatchOutcome struct {
	SnapshotHash   string             `json:"snapshotHash"`
	AtMs           int64              `json:"atMs"`
	TotalAccepted  int                `json:"totalAccepted"`
	TotalDuplicate int                `json:"totalDuplicate"`
	TotalOffline   int                `json:"totalOffline"`
	TotalErrored   int                `json:"totalErrored"`
	PerTarget      []perTargetOutcome `json:"perTarget"`
}

// resendOutcome mirrors `ResendResponse`.
type resendOutcome struct {
	Accepted  int `json:"accepted"`
	Duplicate int `json:"duplicate"`
	Offline   int `json:"offline"`
	Errored   int `json:"errored"`
	Pending   int `json:"pending"`
}

func runSend(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	resendID := ctx.String("resend")
	resend := ctx.Has("resend")
	positional, hasPositional := ctx.Arg(0)

	if !asJSON {
		if resend {
			ui.Header("Resend clipboard entry")
		} else {
			ui.Header("Send clipboard")
		}
	}
	if resend && hasPositional {
		ui.Error("--resend cannot be combined with positional text.")
		return exitcode.Error
	}

	var input sendInput
	if !resend {
		resolved, err := classifySendInput(positional, hasPositional, ctx.Bool("file"), ctx.Bool("text"))
		if err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
		switch resolved.kind {
		case sendInputStdin:
			text, err := readPlaintextStdin()
			if err != nil {
				ui.Error(err.Error())
				return exitcode.Error
			}
			resolved = sendInput{kind: sendInputText, text: text}
		case sendInputStdinFiles:
			paths, err := readFilePathsFromStdin()
			if err != nil {
				ui.Error(err.Error())
				return exitcode.Error
			}
			resolved = sendInput{kind: sendInputFiles, paths: paths}
		}
		if resolved.kind == sendInputText && resolved.text == "" {
			ui.Error("Empty plaintext — nothing to send.")
			return exitcode.Error
		}
		input = resolved
	}

	peers := ctx.Strings("peer")
	var peerFilter []string
	if len(peers) > 0 {
		peerFilter = peers
	}

	// One absolute deadline bounds daemon readiness and target connection so
	// no stage can restart the clock. Dispatch happens at most once, after
	// this phase, so a wait can never duplicate a send.
	timeoutSecs := ctx.Uint("connect-timeout")
	var deadline time.Time
	if timeoutSecs > 0 {
		deadline = time.Now().Add(time.Duration(timeoutSecs) * time.Second)
	}
	var waitPeers []string
	waitForTargets := !deadline.IsZero() && !resend
	if waitForTargets {
		waitPeers = peers
	}

	// The Rust CLI installs its Ctrl-C handler on the first wait and keeps it
	// for the rest of the process: an interrupt during dispatch is swallowed,
	// and only the explicit wait points react to it.
	interrupts := make(chan os.Signal, 1)
	signal.Notify(interrupts, os.Interrupt)

	client, lease, code, cancelled := prepareSendDispatch(interrupts, deadline, waitForTargets, waitPeers, timeoutSecs)
	if cancelled {
		ui.Warn("Cancelled before dispatch; nothing was sent.")
		return exitcode.Error
	}
	if code != exitcode.Success {
		return code
	}
	// Hold the control lease to the end of the command so a transient oneshot
	// daemon does not self-terminate mid-fan-out.
	defer lease.Release()

	switch {
	case resend:
		return runResendViaDaemon(client, resendID, peerFilter, asJSON)
	case input.kind == sendInputFile:
		return runSendFileViaDaemon(client, input.paths[0], peerFilter, asJSON, true, interrupts).exitCode
	case input.kind == sendInputFiles:
		return runSendFilesViaDaemon(client, input.paths, peerFilter, asJSON, interrupts)
	default:
		return runSendTextViaDaemon(client, input.text, peerFilter, asJSON)
	}
}

type preparedDispatch struct {
	client *daemonclient.Client
	lease  *daemonclient.Lease
	code   int
}

// prepareSendDispatch connects to the daemon (spawning a transient one when
// absent), takes the control lease, then optionally waits for the targets.
// It races the whole phase against Ctrl-C.
func prepareSendDispatch(interrupts <-chan os.Signal, deadline time.Time, waitForTargets bool, peers []string, budgetSecs uint64) (*daemonclient.Client, *daemonclient.Lease, int, bool) {
	done := make(chan preparedDispatch, 1)
	go func() {
		client, err := session.ConnectOrSpawnOneshot(deadline)
		if err != nil {
			done <- preparedDispatch{code: session.ExitCode(err)}
			return
		}
		lease, err := client.HoldLease(context.Background())
		if err != nil {
			ui.Error("Failed to hold daemon session lease: " + err.Error())
			done <- preparedDispatch{code: exitcode.Error}
			return
		}
		if waitForTargets {
			if code := waitForSendTargets(peers, deadline, budgetSecs); code != exitcode.Success {
				lease.Release()
				done <- preparedDispatch{code: code}
				return
			}
		}
		done <- preparedDispatch{client: client, lease: lease, code: exitcode.Success}
	}()
	select {
	case <-interrupts:
		return nil, nil, exitcode.Error, true
	case p := <-done:
		return p.client, p.lease, p.code, false
	}
}

// classifySendInput mirrors `classify_input`.
func classifySendInput(value string, present, forceFile, forceText bool) (sendInput, error) {
	if !present {
		if forceFile {
			return sendInput{kind: sendInputStdinFiles}, nil
		}
		return sendInput{kind: sendInputStdin}, nil
	}
	if forceFile {
		return classifySendFile(value)
	}
	if forceText {
		return sendInput{kind: sendInputText, text: value}, nil
	}
	info, err := os.Stat(value)
	switch {
	case err == nil && info.Mode().IsRegular():
		return classifySendFile(value)
	case err == nil && info.IsDir():
		return sendInput{}, fmt.Errorf("Directory sending is not supported: %s", value)
	case err == nil:
		return sendInput{}, fmt.Errorf("Path is not a regular file: %s", value)
	case isNotFound(err) && looksLikePath(value):
		return sendInput{}, fmt.Errorf("Path does not exist: %s", value)
	case isNotFound(err):
		return sendInput{kind: sendInputText, text: value}, nil
	default:
		return sendInput{}, fmt.Errorf("Failed to inspect path %s: %s", value, rustIOError(err))
	}
}

// classifySendFile mirrors `classify_file`: the path must be a readable
// regular file; it resolves to its canonical absolute path.
func classifySendFile(path string) (sendInput, error) {
	info, err := os.Stat(path)
	if err != nil {
		return sendInput{}, fmt.Errorf("Failed to inspect file %s: %s", path, rustIOError(err))
	}
	if info.IsDir() {
		return sendInput{}, fmt.Errorf("Directory sending is not supported: %s", path)
	}
	if !info.Mode().IsRegular() {
		return sendInput{}, fmt.Errorf("Path is not a regular file: %s", path)
	}
	file, err := os.Open(path)
	if err != nil {
		return sendInput{}, fmt.Errorf("File is not readable %s: %s", path, rustIOError(err))
	}
	file.Close()
	canonical, err := canonicalizePath(path)
	if err != nil {
		return sendInput{}, fmt.Errorf("Failed to resolve file path %s: %s", path, rustIOError(err))
	}
	return sendInput{kind: sendInputFile, paths: []string{canonical}}, nil
}

// canonicalizePath mirrors `Path::canonicalize` (realpath).
func canonicalizePath(path string) (string, error) {
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	return filepath.EvalSymlinks(abs)
}

func looksLikePath(raw string) bool {
	return filepath.IsAbs(raw) ||
		strings.HasPrefix(raw, "./") || strings.HasPrefix(raw, "../") ||
		strings.HasPrefix(raw, `.\`) || strings.HasPrefix(raw, `..\`) ||
		strings.Contains(raw, "/") || strings.Contains(raw, `\`)
}

// readStdinUTF8 mirrors `Read::read_to_string` on stdin: the whole stream is
// buffered and must be valid UTF-8.
func readStdinUTF8() (string, error) {
	data, err := io.ReadAll(os.Stdin)
	if err != nil {
		return "", errors.New(rustIOError(err))
	}
	if !utf8.Valid(data) {
		return "", errors.New("stream did not contain valid UTF-8")
	}
	return string(data), nil
}

// readPlaintextStdin reads text until EOF and trims a single trailing
// newline so `echo foo | send` matches `send foo`.
func readPlaintextStdin() (string, error) {
	text, err := readStdinUTF8()
	if err != nil {
		return "", fmt.Errorf("read stdin failed: %v", err)
	}
	if strings.HasSuffix(text, "\n") {
		text = strings.TrimSuffix(text, "\n")
		text = strings.TrimSuffix(text, "\r")
	}
	return text, nil
}

func readFilePathsFromStdin() ([]string, error) {
	if term.IsTerminal(int(os.Stdin.Fd())) {
		return nil, errors.New("File mode needs a path argument or file paths piped through stdin.")
	}
	text, err := readStdinUTF8()
	if err != nil {
		return nil, fmt.Errorf("Failed to read file paths from stdin: %v", err)
	}
	return parseSendFilePaths(text)
}

// parseSendFilePaths takes one complete path per line (CRLF tolerated, blank
// lines ignored, duplicates dropped) and rejects the batch on any bad path.
func parseSendFilePaths(text string) ([]string, error) {
	var paths []string
	seen := map[string]bool{}
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSuffix(line, "\r")
		if line == "" {
			continue
		}
		resolved, err := classifySendFile(line)
		if err != nil {
			return nil, err
		}
		path := resolved.paths[0]
		if !seen[path] {
			seen[path] = true
			paths = append(paths, path)
		}
	}
	if len(paths) == 0 {
		return nil, errors.New("No file paths were provided on stdin.")
	}
	return paths, nil
}

type dispatchRequest struct {
	Text  string   `json:"text"`
	Peers []string `json:"peers"`
}

type resendRequest struct {
	EntryID string   `json:"entryId"`
	Peers   []string `json:"peers"`
}

func runSendTextViaDaemon(client *daemonclient.Client, text string, peers []string, asJSON bool) int {
	spinner := ui.NewSpinner("Dispatching to online peers via daemon...")
	var resp dispatchOutcome
	err := client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/clipboard/dispatch",
		JSON:   dispatchRequest{Text: text, Peers: peers},
	}, &resp)
	if err != nil {
		spinner.FinishError("Dispatch failed: " + err.Error())
		return exitcode.Error
	}
	if resp.PerTarget == nil {
		resp.PerTarget = []perTargetOutcome{}
	}
	spinner.FinishSuccess(fmt.Sprintf("%d accepted, %d duplicate, %d offline, %d error(s)",
		resp.TotalAccepted, resp.TotalDuplicate, resp.TotalOffline, resp.TotalErrored))
	if asJSON {
		if rendered, err := output.Pretty(resp); err == nil {
			fmt.Println(rendered)
		}
	} else {
		renderDaemonDispatch(&resp)
	}
	if resp.TotalAccepted == 0 && resp.TotalDuplicate == 0 {
		return exitcode.Error
	}
	return exitcode.Success
}

func runResendViaDaemon(client *daemonclient.Client, entryID string, peers []string, asJSON bool) int {
	spinner := ui.NewSpinner("Resending entry via daemon...")
	var resp resendOutcome
	err := client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/clipboard/resend",
		JSON:   resendRequest{EntryID: entryID, Peers: peers},
	}, &resp)
	if err != nil {
		spinner.FinishError("Resend failed: " + err.Error())
		return exitcode.Error
	}
	summary := fmt.Sprintf("%d accepted, %d duplicate, %d offline, %d error(s), %d pending",
		resp.Accepted, resp.Duplicate, resp.Offline, resp.Errored, resp.Pending)
	spinner.FinishSuccess(summary)
	if asJSON {
		if rendered, err := output.Pretty(resp); err == nil {
			fmt.Println(rendered)
		}
	} else {
		ui.Bar()
		ui.Info("entry", entryID)
		ui.Info("summary", summary)
		ui.Bar()
	}
	if resp.Accepted == 0 && resp.Duplicate == 0 && resp.Pending == 0 {
		return exitcode.Error
	}
	return exitcode.Success
}

func renderDaemonDispatch(resp *dispatchOutcome) {
	ui.Bar()
	ui.Info("hash", shortHash(resp.SnapshotHash))
	if len(resp.PerTarget) == 0 {
		ui.Info("targets", "(none — no online peers)")
	} else {
		for _, target := range resp.PerTarget {
			var detail string
			switch target.Outcome {
			case "accepted":
				detail = "accepted"
			case "duplicate":
				detail = "duplicate (peer already had it)"
			default:
				reason := "unknown"
				if target.Error != nil {
					reason = *target.Error
				}
				detail = "failed: " + reason
			}
			ui.Info("·", fmt.Sprintf("%s → %s", target.DeviceID, detail))
		}
	}
	ui.Bar()
}

func shortHash(hash string) string {
	if len(hash) > 16 {
		return hash[:16]
	}
	return hash
}
