//go:build !e2e

package main

import (
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
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
