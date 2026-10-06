package main

import (
	"context"
	"log"
	"net/http"
	"os"
	"path/filepath"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/autostart"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// productName names the login item of the primary (profile-less) app. It is injected at build time from the Tauri configuration so both
// shells register the same item; the fallback only serves `go run` and tests.
var productName = "UniClipboard"

func loginItem() (autostart.Registration, error) {
	exe, err := os.Executable()
	if err != nil {
		return autostart.Registration{}, err
	}
	if resolved, err := filepath.EvalSymlinks(exe); err == nil {
		exe = resolved
	}
	name := productName
	if profile, ok := apppaths.Profile(); ok {
		// A named profile is a development or test instance: it gets its own login item so it can
		// never rewrite or remove the one the installed app registered.
		name += "-" + profile
	}
	return autostart.Registration{Name: name, Executable: exe}, nil
}

func (h *HostService) autoStartSetting(ctx context.Context) (bool, error) {
	var settings struct {
		General struct {
			AutoStart bool `json:"autoStart"`
		} `json:"general"`
	}
	err := h.client.Get(ctx, "/settings", &settings)
	return settings.General.AutoStart, err
}

func (h *HostService) patchAutoStart(ctx context.Context, enabled bool) error {
	patch := map[string]any{"general": map[string]any{"autoStart": enabled}}
	return h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil)
}

// updateAutoStart persists the preference first (the stored preference is the source of truth), then
// applies the OS registration; if that fails the preference is rolled back so it never claims a state
// the OS did not reach.
func (h *HostService) updateAutoStart(ctx context.Context, enabled bool) error {
	previous, err := h.autoStartSetting(ctx)
	if err != nil {
		return internalError(err)
	}
	if err := h.patchAutoStart(ctx, enabled); err != nil {
		return internalError(err)
	}
	item, err := loginItem()
	if err == nil {
		err = item.Reconcile(enabled)
	}
	if err != nil {
		if rollback := h.patchAutoStart(ctx, previous); rollback != nil {
			log.Printf("failed to roll back autoStart after the OS registration failed: %v", rollback)
		}
		return commandError{Code: "InternalError", Message: "Failed to apply OS autostart: " + err.Error()}
	}
	return nil
}

// reconcileAutoStart makes the OS registration follow the stored preference at startup. When enabled it
// rewrites the entry to the current executable, healing stale entries from older installs or moved
// binaries. An unreadable setting leaves the OS untouched: its default is `false`, and acting on it
// would remove a login item the user had enabled.
func (h *HostService) reconcileAutoStart() {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	enabled, err := h.autoStartSetting(ctx)
	if err != nil {
		log.Printf("skipping OS autostart reconcile: settings failed to load: %v", err)
		return
	}
	item, err := loginItem()
	if err == nil {
		err = item.Reconcile(enabled)
	}
	if err != nil {
		log.Printf("failed to reconcile OS autostart on startup: %v", err)
	}
}

func init() {
	register(map[string]commandFunc{
		"update_autostart": func(ctx context.Context, h *HostService, args commandArgs) (any, error) {
			var enabled bool
			if err := args.decode("enabled", &enabled); err != nil {
				return nil, err
			}
			return nil, h.updateAutoStart(ctx, enabled)
		},
	})
}
