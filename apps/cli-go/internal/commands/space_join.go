package commands

import (
	"context"
	"net/http"
	"os"
	"os/signal"
	"strings"
	"time"
	"unicode"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	joinPollInterval         = 250 * time.Millisecond
	joinReconnectMaxInterval = 5 * time.Second
	// codeBodyLen is the number of digits in an invitation code body.
	codeBodyLen = 6
)

// normalizeInvitationCode formats six ASCII digits as `XXX-XXX`; any other
// input is only compacted (whitespace and `-` removed, ASCII uppercased)
// for the daemon to reject.
func normalizeInvitationCode(raw string) string {
	var b strings.Builder
	for _, c := range raw {
		if unicode.IsSpace(c) || c == '-' {
			continue
		}
		if c >= 'a' && c <= 'z' {
			c -= 'a' - 'A'
		}
		b.WriteRune(c)
	}
	compact := b.String()
	if len(compact) != codeBodyLen {
		return compact
	}
	for i := 0; i < len(compact); i++ {
		if compact[i] < '0' || compact[i] > '9' {
			return compact
		}
	}
	return compact[:codeBodyLen/2] + "-" + compact[codeBodyLen/2:]
}

type joinArgs struct {
	code, passphrase, deviceName       *string
	switchSpace, yes, preserve, noWait bool
}

func runJoin(ctx *cli.Context) int {
	warnLegacySpaceCommand("join", "space join")
	return runSpaceJoin(ctx)
}

// runSpaceJoin redeems an invitation (default) or, with `--switch`, moves
// this device to a different space. The route follows explicit intent, not
// the local setup state, so a same-space re-pair is never destructive.
func runSpaceJoin(ctx *cli.Context) int {
	args := joinArgs{
		code:        optionalString(ctx, "code"),
		passphrase:  optionalString(ctx, "passphrase"),
		deviceName:  optionalString(ctx, "device-name"),
		switchSpace: ctx.Bool("switch"),
		yes:         ctx.Bool("yes"),
		preserve:    ctx.Bool("preserve-unreadable-history"),
		noWait:      ctx.Bool("no-wait"),
	}
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Join a space")
	}

	var code string
	switch {
	case args.code != nil && strings.TrimSpace(*args.code) != "":
		code = normalizeInvitationCode(*args.code)
	case args.code != nil:
		ui.Error("--code is empty")
		return exitcode.Error
	default:
		entered, err := ui.Password("Invitation code")
		if err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
		if strings.TrimSpace(entered) == "" {
			ui.Error("Invitation code cannot be empty")
			return exitcode.Error
		}
		code = normalizeInvitationCode(entered)
	}

	var passphrase string
	switch {
	case args.passphrase != nil && strings.TrimSpace(*args.passphrase) != "":
		passphrase = *args.passphrase
	case args.passphrase != nil:
		ui.Error("--passphrase is empty")
		return exitcode.Error
	default:
		entered, err := ui.Password("Space passphrase")
		if err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
		if strings.TrimSpace(entered) == "" {
			ui.Error("Passphrase cannot be empty")
			return exitcode.Error
		}
		passphrase = entered
	}

	if args.switchSpace {
		if args.deviceName != nil {
			ui.Warn("--device-name is ignored when switching spaces")
		}
		return runSwitch(code, passphrase, args.yes, args.preserve, args.noWait, asJSON)
	}
	// The parser already requires --switch; kept for parity with the Rust guard.
	if args.preserve {
		ui.Error("--preserve-unreadable-history requires --switch")
		return exitcode.Error
	}
	return runRedeem(code, passphrase, args.deviceName, args.noWait, asJSON)
}

// waitsForJoin mirrors `should_wait_for_join`.
func waitsForJoin(r *joinSpaceResponse, noWait bool) bool {
	return !noWait && (r.Status == joinPending || r.Status == joinProcessing)
}

// runRedeem is the first-time join / re-pair path (`POST /v2/setup/redeem`).
func runRedeem(code, passphrase string, deviceNameArg *string, noWait, asJSON bool) int {
	var deviceName string
	if deviceNameArg != nil && strings.TrimSpace(*deviceNameArg) != "" {
		deviceName = strings.TrimSpace(*deviceNameArg)
	} else if name, ok := session.DefaultDeviceName(); ok {
		deviceName = name
	} else {
		ui.Error("Device name is required (pass --device-name or set a system hostname)")
		return exitcode.Error
	}

	lease, client, err := session.ConnectSetupWithLease(true)
	if err != nil {
		return session.ExitCode(err)
	}
	defer func() { lease.Release() }()

	// The redeem request has no device name; the daemon reads it from settings.
	bg := context.Background()
	patch := map[string]any{"general": map[string]any{"deviceName": deviceName}}
	if err := client.Empty(bg, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}); err != nil {
		ui.Warn("Failed to set device name: " + err.Error())
	}

	spinner := ui.NewSpinner("Dialing sponsor and running handshake...")
	sigCtx, stop := signal.NotifyContext(bg, os.Interrupt)
	defer stop()
	resp, err := decodeJoinResponse(sigCtx, client, daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/v2/setup/redeem",
		JSON:   map[string]string{"code": code, "passphrase": passphrase},
	})
	exit := func() int {
		if sigCtx.Err() != nil {
			return -1
		}
		if err != nil {
			clearSpinner(spinner)
			return renderJoinError("Join failed", err, asJSON)
		}
		if waitsForJoin(resp, noWait) {
			return waitForJoin(sigCtx, &lease, client, spinner, resp.JoinID, "Joined space", &deviceName, asJSON)
		}
		return renderJoinResponse(resp, spinner, "Joined space", &deviceName, asJSON, intentStart)
	}()
	if exit < 0 {
		clearSpinner(spinner)
		return emitJoinError(asJSON, "interrupted", "Stopped waiting; the join operation may still continue in Engine.", exitSigint)
	}
	return exit
}

// runSwitch moves an already set-up device to another space, re-encrypting
// local history. Destructive, so it confirms unless `--yes`.
func runSwitch(code, newPassphrase string, yes, preserve, noWait, asJSON bool) int {
	if !asJSON {
		ui.Warn("This device is already in a space. Switching will re-encrypt all local clipboard history under the new space's master key.")
	}
	if !yes {
		confirmed, err := ui.Confirm("Switch to the new space now?", false)
		if err != nil {
			ui.Error(err.Error())
			return exitcode.Error
		}
		if !confirmed {
			ui.End("Cancelled — staying in the current space.")
			return exitcode.Success
		}
	}

	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer func() { lease.Release() }()

	spinner := ui.NewSpinner("Migrating local clipboard history to the new space (4 phases — this may take a while)...")
	sigCtx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()
	resp, err := decodeJoinResponse(sigCtx, client, daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/v2/setup/switch-space",
		JSON: struct {
			Code                      string `json:"code"`
			NewPassphrase             string `json:"newPassphrase"`
			PreserveUnreadableHistory bool   `json:"preserveUnreadableHistory"`
		}{code, newPassphrase, preserve},
	})
	exit := func() int {
		if sigCtx.Err() != nil {
			return -1
		}
		if err != nil {
			clearSpinner(spinner)
			return renderJoinError("Switch-space failed", err, asJSON)
		}
		if waitsForJoin(resp, noWait) {
			return waitForJoin(sigCtx, &lease, client, spinner, resp.JoinID, "Switched space", nil, asJSON)
		}
		return renderJoinResponse(resp, spinner, "Switched space", nil, asJSON, intentStart)
	}()
	if exit < 0 {
		clearSpinner(spinner)
		return emitJoinError(asJSON, "interrupted", "Stopped waiting; the space migration may still continue in Engine.", exitSigint)
	}
	return exit
}

// waitForJoin polls the device-group choices until the expected join leaves
// pending/processing, reconnecting (with a fresh lease) when the daemon
// drops. It returns -1 when ctx is cancelled by Ctrl+C.
func waitForJoin(ctx context.Context, lease **daemonclient.Lease, client *daemonclient.Client, spinner *ui.Spinner, expectedJoinID, completedMessage string, deviceName *string, asJSON bool) int {
	const pendingMessage = "Join request pending; waiting for final status..."
	spinner.SetMessage(pendingMessage)
	reconnecting := false
	reconnectDelay := joinPollInterval
	for {
		if !sleepCtx(ctx, joinPollInterval) {
			return -1
		}
		current, err := queryCurrentJoin(ctx, client)
		if ctx.Err() != nil {
			return -1
		}
		if err != nil {
			if !reconnecting {
				spinner.SetMessage("Daemon connection interrupted; reconnecting...")
				reconnecting = true
			}
			for {
				if !sleepCtx(ctx, reconnectDelay) {
					return -1
				}
				newLease, newClient, err := session.ConnectSetupWithLease(false)
				if err == nil {
					(*lease).Release()
					*lease, client = newLease, newClient
					reconnectDelay = joinPollInterval
					break
				}
				reconnectDelay = min(reconnectDelay*2, joinReconnectMaxInterval)
			}
			continue
		}
		if reconnecting {
			spinner.SetMessage(pendingMessage)
			reconnecting = false
		}
		switch {
		case current == nil:
			clearSpinner(spinner)
			return emitJoinError(asJSON, "join_status_missing", "The pending join is no longer available. Run `uniclip space join status`.", exitcode.Error)
		case current.JoinID != expectedJoinID:
			clearSpinner(spinner)
			return emitJoinError(asJSON, "join_replaced", "A newer join request replaced this one. Run `uniclip space join status`.", exitcode.Error)
		case current.Status == joinPending || current.Status == joinProcessing:
			continue
		}
		return renderJoinResponse(current, spinner, completedMessage, deviceName, asJSON, intentStart)
	}
}
