package commands

import (
	"context"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

type statusResult struct {
	OK     bool   `json:"ok"`
	Status string `json:"status"`
}

// runSpaceReset rebuilds the space (`POST /v2/setup/reset`). The parser
// already requires `--yes`.
func runSpaceReset(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Rebuild space")
		ui.Warn("All existing device relationships will be permanently discarded.")
		ui.Info("kept", "Local clipboard history, completed files, settings, device identity, unlock access, and the current passphrase.")
	}
	lease, client, err := session.ConnectWithLease()
	if err != nil {
		return session.ExitCode(err)
	}
	defer lease.Release()
	if err := client.Empty(context.Background(), daemonclient.Request{Method: http.MethodPost, Path: "/v2/setup/reset"}); err != nil {
		ui.Error("Failed to rebuild space: " + daemonclient.DisplayMessage(err))
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(statusResult{OK: true, Status: "rebuilt"}, "space rebuild result")
	}
	ui.Success("Space rebuilt. Local history was kept; pair every device again.")
	ui.Info("note", "The passphrase is unchanged. To change it, run `uniclip space change-passphrase`.")
	return exitcode.Success
}
