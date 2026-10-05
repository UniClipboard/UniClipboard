package commands

import (
	"context"
	"fmt"
	"net/http"
	"strconv"
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/daemonclient"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type mobileAddResult struct {
	DeviceID    string `json:"device_id"`
	Label       string `json:"label"`
	BaseURL     string `json:"base_url"`
	Username    string `json:"username"`
	Password    string `json:"password"`
	InstallURL  string `json:"install_url"`
	QRCodeASCII string `json:"qr_code_ascii"`
}

// runMobileAdd registers another iPhone and prints its install QR.
func runMobileAdd(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Add iPhone (SyncClipboard EX)")
	}
	var password *string
	if ctx.Bool("password-stdin") {
		p, err := readPasswordStdin()
		if err != nil {
			ui.Error("Failed to read password from stdin: " + err.Error())
			return exitcode.Error
		}
		password = &p
	}

	s, code := enterMobile("", true)
	if s == nil {
		return code
	}
	defer s.close()

	req := registerMobileDeviceRequest{Label: ctx.String("label"), Password: password}
	if ctx.Has("username") {
		u := ctx.String("username")
		req.Username = &u
	}
	reg, err := s.registerDevice(req)
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(mobileAddResult{
			DeviceID: reg.DeviceID, Label: reg.Label, BaseURL: reg.BaseURL, Username: reg.Username,
			Password: reg.Password, InstallURL: reg.InstallURL, QRCodeASCII: reg.QRCodeASCII,
		}, "mobile add result")
	}
	printRegistration(reg)
	ui.Warn("The password above will NOT be shown again. Copy it now.")
	ui.Warn("Run `uniclip start` so the LAN listener accepts requests from this device.")
	return exitcode.Success
}

type mobileRevokeResult struct {
	DeviceID string `json:"device_id"`
	Revoked  bool   `json:"revoked"`
}

// runMobileRevoke unpairs a device, picking it interactively when no id is
// given (JSON mode requires the id).
func runMobileRevoke(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	s, code := enterMobile("Revoke iPhone device", asJSON)
	if s == nil {
		return code
	}
	defer s.close()

	target, ok := ctx.Arg(0)
	if !ok {
		if asJSON {
			ui.Error("`<device-id>` is required in --json mode.")
			return exitcode.Error
		}
		var code int
		if target, code = pickMobileDevice(s); code != exitcode.Success {
			return code
		}
	}

	if err := revokeMobileDevice(s.client, target); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(mobileRevokeResult{DeviceID: target, Revoked: true}, "mobile revoke result")
	}
	ui.Success(fmt.Sprintf("Revoked device %s.", target))
	ui.Info("note", "Next request from that device returns 401.")
	return exitcode.Success
}

func revokeMobileDevice(client *daemonclient.Client, deviceID string) error {
	segment, err := daemonclient.PathSegment(deviceID)
	if err != nil {
		return err
	}
	return client.Enveloped(context.Background(), daemonclient.Request{
		Method: http.MethodDelete, Path: "/mobile-sync/devices/" + segment,
	}, &struct {
		Success bool `json:"success"`
	}{})
}

// pickMobileDevice lists paired devices on stderr and asks for a number.
func pickMobileDevice(s *mobileSession) (string, int) {
	devices, err := s.listDevices()
	if err != nil {
		ui.Error(err.Error())
		return "", exitcode.Error
	}
	if len(devices) == 0 {
		ui.Warn("No paired devices to revoke.")
		return "", exitcode.Error
	}
	ui.Info("Paired devices", "")
	for i, d := range devices {
		ui.Info(fmt.Sprintf("    %d", i+1), fmt.Sprintf("%s (id=%s)", d.Label, d.DeviceID))
	}
	for {
		v, err := ui.Input(fmt.Sprintf("Pick device [1-%d]", len(devices)), true)
		if err != nil {
			return "", exitcode.Error
		}
		trimmed := strings.TrimSpace(v)
		if trimmed == "" {
			ui.Warn("Aborted by user.")
			return "", exitcode.Error
		}
		// Rust's `usize` parse accepts one leading `+`.
		if n, err := strconv.ParseUint(strings.TrimPrefix(trimmed, "+"), 10, 64); err == nil && n >= 1 && n <= uint64(len(devices)) {
			return devices[n-1].DeviceID, exitcode.Success
		}
		ui.Warn(fmt.Sprintf("Invalid choice; expected 1..%d", len(devices)))
	}
}
