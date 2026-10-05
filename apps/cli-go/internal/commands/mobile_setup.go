package commands

import (
	"strings"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type mobileSetupResult struct {
	DeviceID    string `json:"device_id"`
	Label       string `json:"label"`
	BaseURL     string `json:"base_url"`
	Username    string `json:"username"`
	Password    string `json:"password"`
	InstallURL  string `json:"install_url"`
	QRCodeASCII string `json:"qr_code_ascii"`
	// Pinned advertise IP when `--ip` was given; null when the QR relies on
	// auto-detected interfaces.
	AdvertiseIP *string `json:"advertise_ip"`
	// Resulting LAN listener port; null means the default.
	Port            *uint16 `json:"port"`
	RestartRequired bool    `json:"restart_required"`
}

// runMobileSetup is the one-shot wizard: enable the feature and the LAN
// listener, register a device, and print the install QR and password.
func runMobileSetup(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Mobile setup")
	}
	// JSON mode is implicitly non-interactive.
	nonInteractive := ctx.Bool("non-interactive") || asJSON

	if !ctx.Bool("accept-network-risk") {
		if nonInteractive {
			ui.Error("--accept-network-risk is required in --non-interactive / --json mode (no interactive prompt).")
			return exitcode.Error
		}
		if !confirmNetworkRisk() {
			return exitcode.Error
		}
	}

	// Read the password before connecting so pipe input never waits on the daemon.
	var cliPassword *string
	if ctx.Bool("password-stdin") {
		p, err := readPasswordStdin()
		if err != nil {
			ui.Error("Failed to read password from stdin: " + err.Error())
			return exitcode.Error
		}
		cliPassword = &p
	}

	if nonInteractive && !ctx.Has("label") {
		ui.Error("--label is required in --non-interactive / --json mode.")
		return exitcode.Error
	}

	s, code := enterMobile("", true)
	if s == nil {
		return code
	}
	defer s.close()

	var label string
	if ctx.Has("label") {
		label = ctx.String("label")
	} else {
		v, err := ui.Input(`Device label (e.g. "My iPhone 15")`, false)
		if err != nil {
			ui.Error("Failed to read label: " + err.Error())
			return exitcode.Error
		}
		label = strings.TrimSpace(v)
	}

	var username *string
	switch {
	case ctx.Has("username"):
		u := ctx.String("username")
		username = &u
	case nonInteractive:
	default:
		v, err := ui.Input("Username (6-32 chars, [A-Za-z0-9_], letter-leading) [Enter for auto]", true)
		if err != nil {
			ui.Error("Failed to read username: " + err.Error())
			return exitcode.Error
		}
		if t := strings.TrimSpace(v); t != "" {
			username = &t
		}
	}

	password := cliPassword
	if password == nil && !nonInteractive {
		v, err := ui.Password("Password (min 8 chars) [Enter for auto]:")
		if err != nil {
			ui.Error("Failed to read password: " + err.Error())
			return exitcode.Error
		}
		if v != "" {
			password = &v
		}
	}

	// `--ip` / `--port` are optional pins, sent only when given so a re-run
	// never resets a previously configured port. The base URL stays untouched.
	req := updateMobileSyncSettingsRequest{Enabled: boolPtr(true), LanListenEnabled: boolPtr(true)}
	if ctx.Has("ip") {
		ip := ctx.String("ip")
		ipRef := &ip
		req.LanAdvertiseIP = &ipRef
	}
	if ctx.Has("port") {
		port := uint16(ctx.Uint("port"))
		portRef := &port
		req.LanPort = &portRef
	}
	upd, err := s.updateSettings(req)
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}

	// Never mint credentials for a listener that failed to bind.
	if reason := upd.LanListenerBindError; reason != nil {
		ui.Error("LAN listener failed to bind: " + *reason + ". Free the port (or pick another with `--port`) and re-run setup.")
		return exitcode.Error
	}

	reg, err := s.registerDevice(registerMobileDeviceRequest{Label: label, Username: username, Password: password})
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}

	if asJSON {
		return output.EmitJSON(mobileSetupResult{
			DeviceID: reg.DeviceID, Label: reg.Label, BaseURL: reg.BaseURL, Username: reg.Username,
			Password: reg.Password, InstallURL: reg.InstallURL, QRCodeASCII: reg.QRCodeASCII,
			AdvertiseIP: upd.LanAdvertiseIP, Port: upd.LanPort, RestartRequired: upd.RestartRequired,
		}, "mobile setup result")
	}
	printRegistration(reg)
	ui.Info("note", "The QR carries every detected network address — the phone tries each until one connects, so no address picking is needed.")
	ui.Warn("The password above will NOT be shown again. Copy it now.")
	if upd.RestartRequired {
		ui.Warn(mobileRestartHint)
	}
	return exitcode.Success
}
