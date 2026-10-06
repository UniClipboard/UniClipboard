//go:build e2e

package main

import (
	"os"
	"strings"
	"sync"
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

var (
	dialogMu    sync.Mutex
	dialogCalls = map[string]int{}
)

// dialogOverride answers a native dialog from the environment so the e2e build can drive file
// choices without a person: UC_GUI_GO_E2E_DIALOG_<KIND> is a `|`-separated list answered in call
// order; an empty entry stands for the user cancelling. Past the end of the list it keeps cancelling.
func dialogOverride(kind string) (string, bool) {
	value, ok := os.LookupEnv("UC_GUI_GO_E2E_DIALOG_" + strings.ToUpper(kind))
	if !ok {
		return "", false
	}
	dialogMu.Lock()
	defer dialogMu.Unlock()
	answers := strings.Split(value, "|")
	n := dialogCalls[kind]
	dialogCalls[kind]++
	if n >= len(answers) {
		return "", true
	}
	return answers[n], true
}

// openerOverride records what would have been opened instead of launching the Finder or a viewer
// on the tester's desktop: one `open|reveal <path>` line per call in UC_GUI_GO_E2E_OPEN_LOG.
func openerOverride(path string, reveal bool) (bool, error) {
	log := os.Getenv("UC_GUI_GO_E2E_OPEN_LOG")
	if log == "" {
		return false, nil
	}
	verb := "open"
	if reveal {
		verb = "reveal"
	}
	f, err := os.OpenFile(log, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return true, err
	}
	defer f.Close()
	_, err = f.WriteString(verb + " " + path + "\n")
	return true, err
}
