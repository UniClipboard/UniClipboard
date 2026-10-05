package commands

import (
	"context"
	"fmt"
	"strconv"

	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/cli"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/exitcode"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/output"
	"github.com/UniClipboard/UniClipboard/apps/cli-go/internal/ui"
)

type mobileSettingsView struct {
	Enabled                bool    `json:"enabled"`
	LanListenEnabled       bool    `json:"lanListenEnabled"`
	LanAdvertiseIP         *string `json:"lanAdvertiseIp"`
	LanPort                *uint16 `json:"lanPort"`
	LanAdvertiseBaseURL    *string `json:"lanAdvertiseBaseUrl"`
	LanListenerError       *string `json:"lanListenerError"`
	ShortcutInstallMethods []struct {
		Method         string  `json:"method"`
		Available      bool    `json:"available"`
		DisabledReason *string `json:"disabledReason"`
	} `json:"shortcutInstallMethods"`
}

type mobileStatusOutput struct {
	Enabled             bool    `json:"enabled"`
	LanListenEnabled    bool    `json:"lan_listen_enabled"`
	LanAdvertiseIP      *string `json:"lan_advertise_ip"`
	LanAdvertiseBaseURL *string `json:"lan_advertise_base_url"`
	LanPort             *uint16 `json:"lan_port"`
	// Bind failure reason reported by the daemon's LAN listener.
	LanListenerError *string `json:"lan_listener_error"`
	// User-facing listen URL derived from the persisted settings.
	ListenURL              string                `json:"listen_url"`
	DeviceCount            int                   `json:"device_count"`
	Devices                []mobileDeviceLine    `json:"devices"`
	ShortcutInstallMethods []mobileInstallMethod `json:"shortcut_install_methods"`
}

type mobileDeviceLine struct {
	DeviceID     string `json:"device_id"`
	Label        string `json:"label"`
	LastSeenAtMs *int64 `json:"last_seen_at_ms"`
}

type mobileInstallMethod struct {
	Method         string  `json:"method"`
	Available      bool    `json:"available"`
	DisabledReason *string `json:"disabled_reason"`
}

// mobileListenURL is the externally advertised address: the base URL when
// set, otherwise `http://<ip or 0.0.0.0>:<port or 42720>`.
func mobileListenURL(v mobileSettingsView) string {
	if v.LanAdvertiseBaseURL != nil {
		return *v.LanAdvertiseBaseURL
	}
	host := "0.0.0.0"
	if v.LanAdvertiseIP != nil {
		host = *v.LanAdvertiseIP
	}
	port := uint16(42720)
	if v.LanPort != nil {
		port = *v.LanPort
	}
	return fmt.Sprintf("http://%s:%d", host, port)
}

func orDefault(v *string, fallback string) string {
	if v != nil {
		return *v
	}
	return fallback
}

// runMobileStatus shows settings, the derived listen URL and paired devices.
func runMobileStatus(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	s, code := enterMobile("Mobile status", asJSON)
	if s == nil {
		return code
	}
	defer s.close()

	var view mobileSettingsView
	if err := s.client.Get(context.Background(), "/mobile-sync/settings", &view); err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	devices, err := s.listDevices()
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}

	if asJSON {
		out := mobileStatusOutput{
			Enabled: view.Enabled, LanListenEnabled: view.LanListenEnabled,
			LanAdvertiseIP: view.LanAdvertiseIP, LanAdvertiseBaseURL: view.LanAdvertiseBaseURL,
			LanPort: view.LanPort, LanListenerError: view.LanListenerError,
			ListenURL: mobileListenURL(view), DeviceCount: len(devices),
			Devices: []mobileDeviceLine{}, ShortcutInstallMethods: []mobileInstallMethod{},
		}
		for _, d := range devices {
			out.Devices = append(out.Devices, mobileDeviceLine{DeviceID: d.DeviceID, Label: d.Label, LastSeenAtMs: d.LastSeenAtMs})
		}
		for _, m := range view.ShortcutInstallMethods {
			out.ShortcutInstallMethods = append(out.ShortcutInstallMethods, mobileInstallMethod{Method: m.Method, Available: m.Available, DisabledReason: m.DisabledReason})
		}
		return output.EmitJSON(out, "mobile status")
	}

	ui.Info("enabled", strconv.FormatBool(view.Enabled))
	ui.Info("lanListenEnabled", strconv.FormatBool(view.LanListenEnabled))
	ui.Info("lanAdvertise", orDefault(view.LanAdvertiseIP, "(none, fallback 0.0.0.0)"))
	ui.Info("lanAdvertiseUrl", orDefault(view.LanAdvertiseBaseURL, "(none, using LAN ip:port)"))
	port := "(none, default 42720)"
	if view.LanPort != nil {
		port = strconv.Itoa(int(*view.LanPort))
	}
	ui.Info("lanPort", port)
	ui.Info("listenUrl", mobileListenURL(view))
	if view.LanListenerError != nil {
		ui.Info("listenerError", *view.LanListenerError)
	}
	ui.Bar()
	if len(devices) == 0 {
		ui.Info("devices", "0 — run `uniclip mobile setup` or `uniclip mobile add` to register one.")
		return exitcode.Success
	}
	ui.Info("devices", fmt.Sprintf("%d paired", len(devices)))
	for _, d := range devices {
		seen := "never"
		if d.LastSeenAtMs != nil {
			seen = strconv.FormatInt(*d.LastSeenAtMs, 10)
		}
		ui.Info("    "+d.Label, fmt.Sprintf("id=%s last_seen_ms=%s", d.DeviceID, seen))
	}
	return exitcode.Success
}

type mobileDisableResult struct {
	Enabled          bool `json:"enabled"`
	LanListenEnabled bool `json:"lan_listen_enabled"`
	RestartRequired  bool `json:"restart_required"`
}

// runMobileDisable turns off the master switch and the LAN listener.
func runMobileDisable(ctx *cli.Context) int {
	asJSON := ctx.JSON()
	s, code := enterMobile("Mobile disable", asJSON)
	if s == nil {
		return code
	}
	defer s.close()

	out, err := s.updateSettings(updateMobileSyncSettingsRequest{Enabled: boolPtr(false), LanListenEnabled: boolPtr(false)})
	if err != nil {
		ui.Error(err.Error())
		return exitcode.Error
	}
	if asJSON {
		return output.EmitJSON(mobileDisableResult{Enabled: out.Enabled, LanListenEnabled: out.LanListenEnabled, RestartRequired: out.RestartRequired}, "mobile disable result")
	}
	ui.Success("Mobile-sync disabled (master switch + LAN listener).")
	ui.Info("note", "Paired devices remain registered. Revoke individually with `uniclip mobile revoke`.")
	if out.RestartRequired {
		ui.Warn(mobileRestartHint)
	} else {
		ui.Info("note", "Already disabled — no daemon restart needed.")
	}
	return exitcode.Success
}
