//go:build e2e

package main

import (
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
)

type devUpdate struct {
	endpoints func(update.Channel) []string
	publicKey string
}

// devUpdateOverrides lets the e2e build point the updater at a local feed signed
// with a throwaway key, mirroring the Tauri shell's debug-only UC_UPDATE_* overrides.
func devUpdateOverrides() (devUpdate, bool) {
	endpoint, key := os.Getenv("UC_UPDATE_ENDPOINT"), os.Getenv("UC_UPDATE_PUBKEY")
	if endpoint == "" || key == "" {
		return devUpdate{}, false
	}
	return devUpdate{endpoints: func(update.Channel) []string { return []string{endpoint} }, publicKey: key}, true
}

// schedulerTimingOverride shortens the scheduler cadence for the e2e build so a
// background check can be observed in seconds; jitter is disabled for determinism.
func schedulerTimingOverride(t schedulerTiming) schedulerTiming {
	if d, err := time.ParseDuration(os.Getenv("UC_UPDATE_SCHEDULER_INTERVAL")); err == nil && d > 0 {
		t.setupPoll, t.success, t.jitter, t.failure = d, d, 0, d
	}
	return t
}

// helperExecutable lets the e2e build substitute a stand-in helper to exercise the supervision
// and request handling deterministically; without the override it is the real helper.
func helperExecutable() (string, bool) {
	if path := os.Getenv("UC_QUICK_PANEL_HELPER_EXE"); path != "" {
		return path, true
	}
	return quickpanelhelper.ResolveExePath()
}
