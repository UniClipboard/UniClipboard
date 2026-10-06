//go:build !e2e

package main

import (
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/wailsapp/wails/v3/pkg/application"
)

type devUpdate struct {
	endpoints func(update.Channel) []string
	publicKey string
}

// devUpdateOverrides is empty in normal builds: the feed and the trusted key
// cannot be redirected by the environment.
func devUpdateOverrides() (devUpdate, bool) { return devUpdate{}, false }

// schedulerTimingOverride keeps the production cadence in normal builds.
func schedulerTimingOverride(t schedulerTiming) schedulerTiming { return t }

// helperExecutable is the quick panel helper next to this executable; normal builds cannot
// redirect it through the environment.
func helperExecutable() (string, bool) { return quickpanelhelper.ResolveExePath() }

// dialogOverride and openerOverride are absent in normal builds: native dialogs and the system
// opener cannot be replaced through the environment.
func dialogOverride(string) (string, bool)      { return "", false }
func openerOverride(string, bool) (bool, error) { return false, nil }

// notifyOverride and notifyPermissionOverride are absent in normal builds: notifications always go to
// the system.
func notifyOverride(string, string, string) (bool, error) { return false, nil }
func notifyPermissionOverride() (bool, bool)              { return false, false }

// keyringUnlockDenied is always false in normal builds: the keychain attempt always reaches the daemon.
func keyringUnlockDenied() bool { return false }

// forceMainWindow is always false in normal builds.
func forceMainWindow() bool { return false }

// Window seams. Normal builds place, focus and activate windows directly; the e2e build keeps them out of
// the tester's way (see updater_dev_e2e.go).
func quietOptions(o application.WebviewWindowOptions) application.WebviewWindowOptions { return o }
func focusWindow(w application.Window)                                                 { w.Focus() }
func moveWindow(w application.Window, x, y int)                                        { w.SetPosition(x, y) }
func centerWindow(w application.Window)                                                { w.Center() }
func activationPolicy() application.ActivationPolicy                                   { return application.ActivationPolicyRegular }
func cursorOverride() (float64, float64, bool)                                         { return 0, 0, false }
