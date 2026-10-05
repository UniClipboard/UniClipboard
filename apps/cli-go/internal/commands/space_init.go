package commands

import (
	"context"
	"net/http"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/session"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type initOutput struct {
	SpaceID     string `json:"space_id"`
	DeviceID    string `json:"device_id"`
	Fingerprint string `json:"fingerprint"`
}

type initializeSpaceRequest struct {
	Passphrase        string  `json:"passphrase"`
	PassphraseConfirm string  `json:"passphraseConfirm"`
	DeviceName        *string `json:"deviceName"`
}

type initializeSpaceResponse struct {
	SpaceID      string `json:"spaceId"`
	SelfDeviceID string `json:"selfDeviceId"`
	Fingerprint  string `json:"fingerprint"`
}

func runInit(ctx *cli.Context) int {
	warnLegacySpaceCommand("init", "space init")
	return runSpaceInit(ctx)
}

// runSpaceInit creates a new encrypted space through `POST /v2/setup/initialize`.
// It is the setup command itself, so it spawns a daemon without the setup gate.
func runSpaceInit(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Initialize space")
	}
	passphrase, code, ok := readNewPassphrase(optionalString(ctx, "passphrase"), asJSON)
	if !ok {
		return code
	}
	deviceName := optionalString(ctx, "device-name")
	if deviceName == nil {
		if name, found := session.DefaultDeviceName(); found {
			deviceName = &name
		}
	}
	if deviceName == nil {
		ui.Error("Device name is required (--device-name or auto-detected hostname)")
		return exitcode.Error
	}

	client, err := session.EnsureDaemonForSetup(true)
	if err != nil {
		return session.ExitCode(err)
	}
	bg := context.Background()
	lease, err := client.HoldLease(bg)
	if err != nil {
		ui.Error("Failed to acquire control lease: " + err.Error())
		return exitcode.Error
	}
	defer lease.Release()

	var spinner *ui.Spinner
	if !asJSON {
		spinner = ui.NewSpinner("Creating encrypted space...")
	}
	var resp initializeSpaceResponse
	err = client.Enveloped(bg, daemonclient.Request{
		Method: http.MethodPost,
		Path:   "/v2/setup/initialize",
		JSON:   initializeSpaceRequest{Passphrase: passphrase, PassphraseConfirm: passphrase, DeviceName: deviceName},
	}, &resp)
	if err != nil {
		message := daemonclient.DisplayMessage(err)
		if spinner != nil {
			spinnerError(spinner, message)
		} else {
			ui.Error(message)
		}
		if re, ok := daemonclient.AsRequestError(err); !asJSON && ok && re.Kind == daemonclient.ErrStatus && re.Status == http.StatusConflict {
			ui.Info("hint", "This device already has a space. To change its passphrase, run `uniclip space change-passphrase`.")
		}
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(initOutput{SpaceID: resp.SpaceID, DeviceID: resp.SelfDeviceID, Fingerprint: resp.Fingerprint}, "space initialization result")
	}
	spinnerSuccess(spinner, "Space initialized")
	ui.Info("space_id", resp.SpaceID)
	ui.Info("device_id", resp.SelfDeviceID)
	ui.Info("fingerprint", resp.Fingerprint)
	return exitcode.Success
}
