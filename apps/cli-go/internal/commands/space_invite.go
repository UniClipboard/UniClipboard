package commands

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

const (
	wsTopicSetup                = "setup"
	wsEventSetupPairingComplete = "setup.pairingCompleted"
)

// inviteEvent is one JSON-lines event of `space invite --json`. Field order
// follows the Rust enum (tag first, then the variant fields).
type inviteEvent struct {
	Event           string  `json:"event"`
	Code            *string `json:"code,omitempty"`
	ExpiresAtMs     *int64  `json:"expires_at_ms,omitempty"`
	SponsorDeviceID *string `json:"sponsor_device_id,omitempty"`
	JoinerDeviceID  *string `json:"joiner_device_id,omitempty"`
	Reason          *string `json:"reason,omitempty"`
}

type issueInvitationResponse struct {
	Code        string `json:"code"`
	ExpiresAtMs int64  `json:"expiresAtMs"`
}

type setupPairingCompletedEvent struct {
	SponsorDeviceID string  `json:"sponsorDeviceId"`
	JoinerDeviceID  *string `json:"joinerDeviceId"`
	Success         bool    `json:"success"`
	Reason          *string `json:"reason"`
}

// inviteOutput renders either the human transcript or JSON lines.
type inviteOutput struct{ json bool }

func (o inviteOutput) spinner(message string) *ui.Spinner {
	if o.json {
		return nil
	}
	return ui.NewSpinner(message)
}

func (o inviteOutput) invitationIssued(spinner *ui.Spinner, code string, expiresAtMs int64) error {
	if o.json {
		clearSpinner(spinner)
		return emitInviteEvent(inviteEvent{Event: "invitation_issued", Code: &code, ExpiresAtMs: &expiresAtMs})
	}
	spinnerSuccess(spinner, "Invitation issued")
	ui.Bar()
	ui.VerificationCode(code)
	ui.Info("expires_at", rfc3339Millis(expiresAtMs))
	ui.Bar()
	if _, err := fmt.Fprintf(os.Stdout, "INVITATION_CODE=%s\n", code); err != nil {
		return fmt.Errorf("Failed to write invitation code: %v", err)
	}
	return nil
}

func (o inviteOutput) requestFailed(spinner *ui.Spinner, message string) {
	if o.json {
		clearSpinner(spinner)
		ui.Error(message)
		return
	}
	spinnerError(spinner, message)
}

func (o inviteOutput) pairingCompleted(spinner *ui.Spinner, sponsorDeviceID string, joinerDeviceID *string) error {
	if o.json {
		clearSpinner(spinner)
		return emitInviteEvent(inviteEvent{Event: "pairing_completed", SponsorDeviceID: &sponsorDeviceID, JoinerDeviceID: joinerDeviceID})
	}
	spinnerSuccess(spinner, "Pairing completed")
	ui.Info("sponsor_device_id", sponsorDeviceID)
	if joinerDeviceID != nil {
		ui.Info("joiner_device_id", *joinerDeviceID)
	}
	return nil
}

func (o inviteOutput) pairingFailed(spinner *ui.Spinner, reason string) error {
	if o.json {
		clearSpinner(spinner)
		return emitInviteEvent(inviteEvent{Event: "pairing_failed", Reason: &reason})
	}
	spinnerError(spinner, "Pairing failed: "+reason)
	return nil
}

func (o inviteOutput) interrupted(spinner *ui.Spinner) error {
	if o.json {
		clearSpinner(spinner)
		return emitInviteEvent(inviteEvent{Event: "interrupted"})
	}
	spinnerError(spinner, "Interrupted by user")
	return nil
}

func emitInviteEvent(event inviteEvent) error {
	line, err := output.Compact(event)
	if err != nil {
		return fmt.Errorf("Failed to serialize invitation event: %v", err)
	}
	if _, err := fmt.Fprintln(os.Stdout, line); err != nil {
		return fmt.Errorf("Failed to write invitation event: %v", err)
	}
	return nil
}

// rfc3339Millis renders epoch millis like chrono's `to_rfc3339()` on a UTC
// timestamp: `+00:00` offset and fractional seconds only when non-zero.
func rfc3339Millis(ms int64) string {
	t := time.UnixMilli(ms).UTC()
	frac := ""
	if millis := t.Nanosecond() / int(time.Millisecond); millis != 0 {
		frac = fmt.Sprintf(".%03d", millis)
	}
	return t.Format("2006-01-02T15:04:05") + frac + "+00:00"
}

func invitationRequestErrorMessage(err error) string {
	switch daemonclient.ErrorCode(err) {
	case "invitation_no_publishable_address":
		return "No usable network connection is available for pairing. Connect this device to a network, then try again."
	case "invitation_local_publication_failed":
		return "This device could not make the invitation available on the local network. Check its network connection, then try again."
	case "invitation_directory_transport_failed":
		return "The invitation service could not be reached, and local pairing is unavailable. Check the network, then try again."
	case "invitation_directory_rejected":
		return "The invitation service declined this request. Do not keep retrying; export diagnostics and contact support."
	case "invitation_directory_invalid_response":
		return "The invitation service returned an invalid response. Try again once; if it continues, export diagnostics and contact support."
	}
	return daemonclient.DisplayMessage(err)
}

func runInvite(ctx *cli.Context) int {
	warnLegacySpaceCommand("invite", "space invite")
	return runSpaceInvite(ctx)
}

// runSpaceInvite issues a pairing invitation and waits for the
// `setup.pairingCompleted` outcome or Ctrl+C. The subscription WebSocket
// also acts as the daemon control lease.
func runSpaceInvite(ctx *cli.Context) int {
	out := inviteOutput{json: ctx.JSON()}
	if !out.json {
		ui.Header("Invite a device")
	}
	client, err := session.ConnectOrSpawnOneshot(time.Time{})
	if err != nil {
		return session.ExitCode(err)
	}

	// Subscribe before issuing so an outcome racing the POST is not missed.
	bg := context.Background()
	ws, err := client.DialWS(bg)
	if err == nil {
		err = ws.Subscribe(bg, wsTopicSetup)
		if err != nil {
			ws.Close()
		}
	}
	if err != nil {
		ui.Error("Failed to subscribe pairing completion: " + err.Error())
		return exitcode.Error
	}
	defer ws.Close()
	outcomes := pairingOutcomes(ws)

	spinner := out.spinner("Requesting invitation from rendezvous...")
	var invitation issueInvitationResponse
	if err := client.Enveloped(bg, daemonclient.Request{Method: http.MethodPost, Path: "/v2/setup/issue-invitation"}, &invitation); err != nil {
		out.requestFailed(spinner, invitationRequestErrorMessage(err))
		return exitcode.Error
	}
	if err := out.invitationIssued(spinner, invitation.Code, invitation.ExpiresAtMs); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}

	waiting := out.spinner("Waiting for joiner to complete handshake (Ctrl+C to cancel)...")
	sigCtx, stop := signal.NotifyContext(bg, os.Interrupt)
	defer stop()
	select {
	case event, ok := <-outcomes:
		if !ok {
			if err := out.pairingFailed(waiting, "outcome stream ended unexpectedly"); err != nil {
				ui.Error(err.Error())
			}
			return exitcode.Error
		}
		if event.Success {
			if err := out.pairingCompleted(waiting, event.SponsorDeviceID, event.JoinerDeviceID); err != nil {
				ui.Error(err.Error())
				return exitcode.Error
			}
			return exitcode.Success
		}
		reason := "unknown"
		if event.Reason != nil {
			reason = *event.Reason
		}
		if err := out.pairingFailed(waiting, reason); err != nil {
			ui.Error(err.Error())
		}
		return exitcode.Error
	case <-sigCtx.Done():
		if err := out.interrupted(waiting); err != nil {
			ui.Error(err.Error())
		}
		return exitSigint
	}
}

// pairingOutcomes filters the setup topic down to decoded
// `setup.pairingCompleted` payloads; the channel closes with the socket.
func pairingOutcomes(ws *daemonclient.WS) <-chan setupPairingCompletedEvent {
	out := make(chan setupPairingCompletedEvent, 64)
	go func() {
		defer close(out)
		for event := range ws.Events(context.Background()) {
			if event.Type != wsEventSetupPairingComplete || len(event.Payload) == 0 {
				continue
			}
			var completed setupPairingCompletedEvent
			if json.Unmarshal(event.Payload, &completed) != nil {
				continue
			}
			out <- completed
		}
	}()
	return out
}
