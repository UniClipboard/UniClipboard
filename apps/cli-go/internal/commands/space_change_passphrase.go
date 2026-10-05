package commands

import (
	"context"
	"encoding/json"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type changePassphraseRequest struct {
	Passphrase             string `json:"passphrase"`
	PassphraseConfirmation string `json:"passphraseConfirmation"`
}

// passphraseRejectionHint suggests a next step for a daemon rejection code.
func passphraseRejectionHint(code string) string {
	switch code {
	case "MULTIPLE_DEVICES":
		return "Remove the other devices with `uniclip member remove`, or rebuild with `uniclip space reset --yes`, then retry."
	case "SPACE_LOCKED":
		return "Unlock this device first, then retry."
	}
	return ""
}

// runSpaceChangePassphrase replaces the space passphrase
// (`POST /encryption/passphrase`); the daemon only accepts it for an
// unlocked, single-device space.
func runSpaceChangePassphrase(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Change space passphrase")
	}
	passphrase, code, ok := readNewPassphrase(optionalString(ctx, "passphrase"), asJSON)
	if !ok {
		return code
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	var ignored json.RawMessage
	err = client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/encryption/passphrase",
		JSON:   changePassphraseRequest{Passphrase: passphrase, PassphraseConfirmation: passphrase},
	}, &ignored)
	if err != nil {
		ui.Error("Failed to change passphrase: " + daemonclient.DisplayMessage(err))
		if hint := passphraseRejectionHint(daemonclient.ErrorCode(err)); !asJSON && hint != "" {
			ui.Info("hint", hint)
		}
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(statusResult{OK: true, Status: "changed"}, "passphrase change result")
	}
	ui.Success("Passphrase changed. Local history was kept.")
	ui.Info("note", "Unused pairing invitations were invalidated; issue a new one with `uniclip space invite`.")
	return exitcode.Success
}
