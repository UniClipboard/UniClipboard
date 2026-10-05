package commands

import (
	"context"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type lanInterface struct {
	Name string `json:"name"`
	IPv4 string `json:"ipv4"`
}

// runMobileNetworkInterfaces lists RFC1918 LAN IPv4 interfaces.
func runMobileNetworkInterfaces(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	s, code := enterMobile("LAN interfaces", asJSON)
	if s == nil {
		return code
	}
	defer s.close()

	interfaces := []lanInterface{}
	if err := s.client.Get(context.Background(), "/mobile-sync/lan-interfaces", &interfaces); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		if interfaces == nil {
			interfaces = []lanInterface{}
		}
		return output.EmitJSON(interfaces, "LAN interfaces")
	}
	if len(interfaces) == 0 {
		ui.Warn("No RFC1918 LAN interface detected. Connect to a private network and retry.")
	}
	for _, o := range interfaces {
		ui.Info(o.Name, o.IPv4)
	}
	return exitcode.Success
}

type mobileNetworkSetResult struct {
	Enabled             bool    `json:"enabled"`
	LanListenEnabled    bool    `json:"lan_listen_enabled"`
	LanAdvertiseIP      *string `json:"lan_advertise_ip"`
	LanAdvertiseBaseURL *string `json:"lan_advertise_base_url"`
	LanPort             *uint16 `json:"lan_port"`
	RestartRequired     bool    `json:"restart_required"`
}

// runMobileNetworkSet points the advertised address at an IP or a base URL
// and turns the LAN listener on.
func runMobileNetworkSet(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	if !asJSON {
		ui.Header("Mobile network set")
	}

	// Exactly one of the two advertise fields gets a value; the other is
	// cleared explicitly. Clap's required group already rejects other shapes.
	var ipValue, urlValue *string
	switch hasIP, hasURL := ctx.Has("ip"), ctx.Has("url"); {
	case hasIP && !hasURL:
		v := ctx.String("ip")
		ipValue = &v
	case hasURL && !hasIP:
		v := ctx.String("url")
		urlValue = &v
	case !hasIP:
		ui.Error("One of --ip <IP> or --url <URL> is required.")
		return exitcode.Error
	default:
		ui.Error("--ip and --url are mutually exclusive.")
		return exitcode.Error
	}

	if !ctx.Bool("accept-network-risk") {
		if asJSON {
			ui.Error("--accept-network-risk is required in JSON mode (no interactive prompt).")
			return exitcode.Error
		}
		if !confirmNetworkRisk() {
			return exitcode.Error
		}
	}

	s, code := enterMobile("", true)
	if s == nil {
		return code
	}
	defer s.close()

	// The port is always sent: absent `--port` clears it back to the default.
	var port *uint16
	if ctx.Has("port") {
		p := uint16(ctx.Uint("port"))
		port = &p
	}
	out, err := s.updateSettings(updateMobileSyncSettingsRequest{
		Enabled: boolPtr(true), LanListenEnabled: boolPtr(true),
		LanAdvertiseIP: &ipValue, LanAdvertiseBaseURL: &urlValue, LanPort: &port,
	})
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(mobileNetworkSetResult{
			Enabled: out.Enabled, LanListenEnabled: out.LanListenEnabled, LanAdvertiseIP: out.LanAdvertiseIP,
			LanAdvertiseBaseURL: out.LanAdvertiseBaseURL, LanPort: out.LanPort, RestartRequired: out.RestartRequired,
		}, "mobile network set result")
	}
	ui.Success("LAN listener enabled in settings.")
	ui.Info("advertise", orDefault(out.LanAdvertiseIP, "(unset)"))
	ui.Info("advertiseUrl", orDefault(out.LanAdvertiseBaseURL, "(unset)"))
	portText := "default (42720)"
	if out.LanPort != nil {
		portText = strconv.Itoa(int(*out.LanPort))
	}
	ui.Info("port", portText)
	if out.RestartRequired {
		ui.Warn(mobileRestartHint)
	}
	return exitcode.Success
}

type mobileNetworkOffResult struct {
	LanListenEnabled bool `json:"lan_listen_enabled"`
	RestartRequired  bool `json:"restart_required"`
}

// runMobileNetworkOff turns off only the LAN listener.
func runMobileNetworkOff(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	s, code := enterMobile("Mobile network off", asJSON)
	if s == nil {
		return code
	}
	defer s.close()

	out, err := s.updateSettings(updateMobileSyncSettingsRequest{LanListenEnabled: boolPtr(false)})
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(mobileNetworkOffResult{LanListenEnabled: out.LanListenEnabled, RestartRequired: out.RestartRequired}, "mobile network off result")
	}
	ui.Success("LAN listener disabled in settings.")
	if out.RestartRequired {
		ui.Warn(mobileRestartHint)
	}
	return exitcode.Success
}
