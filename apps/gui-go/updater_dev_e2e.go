//go:build e2e

package main

import (
	"os"
	"strings"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/wailsapp/wails/v3/pkg/application"
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

// notifyOverride records a notification instead of showing it (the real service needs the user's
// permission and would pop UI on the tester's desktop): one `id|title|body` line per call, newlines in
// the body escaped, in UC_GUI_GO_E2E_NOTIFY_LOG.
func notifyOverride(id, title, body string) (bool, error) {
	log := os.Getenv("UC_GUI_GO_E2E_NOTIFY_LOG")
	if log == "" {
		return false, nil
	}
	f, err := os.OpenFile(log, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		return true, err
	}
	defer f.Close()
	_, err = f.WriteString(id + "|" + title + "|" + strings.ReplaceAll(body, "\n", "\\n") + "\n")
	return true, err
}

// notifyPermissionOverride answers the permission questions as granted while the recorder is active.
func notifyPermissionOverride() (bool, bool) {
	return true, os.Getenv("UC_GUI_GO_E2E_NOTIFY_LOG") != ""
}

// forceMainWindow builds the main window even for a Silent or Lightweight launch, so the setup launches of
// the startup-mode scenarios (which need a page to run their driver in) work whatever mode is stored.
func forceMainWindow() bool {
	return strings.HasPrefix(os.Getenv("UC_GUI_GO_E2E_PHASE"), "startup-set")
}

// Quiet mode keeps a test run off the tester's desktop: the app is an accessory (no Dock icon, no focus
// stealing), windows are created far off-screen and never moved or focused, and the pointer is never touched
// (the quick panel's cursor position is injected). Placement is therefore recorded rather than applied, and
// the assertions read the recorded position. UC_GUI_GO_E2E_VISIBLE=1 turns it off for watching a run.
func quiet() bool { return os.Getenv("UC_GUI_GO_E2E_VISIBLE") != "1" }

const offscreen = -20000

func quietOptions(o application.WebviewWindowOptions) application.WebviewWindowOptions {
	if quiet() {
		o.InitialPosition, o.X, o.Y = application.WindowXY, offscreen, offscreen
	}
	return o
}

func focusWindow(w application.Window) {
	if !quiet() {
		w.Focus()
	}
}

var (
	placedMu sync.Mutex
	placed   = map[string][2]int{}
)

// moveWindow places a window, or in quiet mode only records where it would have gone.
func moveWindow(w application.Window, x, y int) {
	if !quiet() {
		w.SetPosition(x, y)
		return
	}
	placedMu.Lock()
	placed[w.Name()] = [2]int{x, y}
	placedMu.Unlock()
}

func centerWindow(w application.Window) {
	if !quiet() {
		w.Center()
	}
}

// placedPosition is where the window is (or, in quiet mode, would be) on screen.
func placedPosition(w application.Window) (int, int) {
	placedMu.Lock()
	defer placedMu.Unlock()
	if p, ok := placed[w.Name()]; ok && quiet() {
		return p[0], p[1]
	}
	return w.Position()
}

func activationPolicy() application.ActivationPolicy {
	if quiet() {
		return application.ActivationPolicyAccessory
	}
	return application.ActivationPolicyRegular
}

var (
	cursorMu       sync.Mutex
	injectedCursor [2]float64
	cursorInjected bool
)

func injectCursor(x, y float64) {
	cursorMu.Lock()
	injectedCursor, cursorInjected = [2]float64{x, y}, true
	cursorMu.Unlock()
}

func cursorOverride() (float64, float64, bool) {
	cursorMu.Lock()
	defer cursorMu.Unlock()
	return injectedCursor[0], injectedCursor[1], cursorInjected && quiet()
}
