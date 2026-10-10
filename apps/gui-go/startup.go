package main

import (
	"context"
	"log"
	"net/http"
	"net/url"
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Startup modes, as stored in `general.startupMode`.
const (
	startupNormal      = "normal"
	startupSilent      = "silent"
	startupLightweight = "lightweight"
)

// startupSettings are the preferences that shape the launch sequence.
type startupSettings struct {
	StartupMode               string `json:"startupMode"`
	RestoreLastEntryOnStartup bool   `json:"restoreLastEntryOnStartup"`
	DeviceName                string `json:"deviceName"`
}

// Silent and Lightweight launches keep the window hidden at boot; Lightweight then decides once it knows
// whether this launch started the daemon.
func (s startupSettings) hidden() bool {
	return s.StartupMode == startupSilent || s.StartupMode == startupLightweight
}
func (s startupSettings) silent() bool      { return s.StartupMode == startupSilent }
func (s startupSettings) lightweight() bool { return s.StartupMode == startupLightweight }

// loadStartupSettings reads the launch preferences. When settings cannot be read the defaults apply (a
// normal launch), as in the Tauri shell.
func (h *HostService) loadStartupSettings() (startupSettings, bool) {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	var settings struct {
		General startupSettings `json:"general"`
	}
	if err := h.daemon().Get(ctx, "/settings", &settings); err != nil {
		log.Printf("failed to load settings for startup: %v; using defaults", err)
		return startupSettings{StartupMode: startupNormal}, false
	}
	return settings.General, true
}

// startupWindowAction is what a launch does with the window once the daemon origin is known.
type startupWindowAction int

const (
	windowNone startupWindowAction = iota
	windowEnterBackgroundOnly
	windowShow
)

// resolveWindowAction: a Lightweight launch that started the daemon (a cold start) hands off to
// background-only running; one that merely attached to a running daemon (a reopen) shows the window.
func resolveWindowAction(spawnedThisLaunch, lightweight bool) startupWindowAction {
	switch {
	case lightweight && spawnedThisLaunch:
		return windowEnterBackgroundOnly
	case lightweight:
		return windowShow
	}
	return windowNone
}

// coldLaunch runs the daemon-dependent startup sequence of crates/uc-desktop/src/startup/actions.rs:
// name the device if it has no name, and on a cold start (this launch spawned the daemon) recover the
// encryption session and lifecycle, then restore the latest clipboard entry when enabled. A reopen skips
// the recovery and the restore: they belong to the daemon's original start, and restoring again would
// overwrite the OS clipboard on every window reopen.
func (h *HostService) coldLaunch(settings startupSettings, spawned bool) {
	h.ensureDeviceName(settings)
	if spawned {
		h.recoverAfterColdLaunch()
		if settings.RestoreLastEntryOnStartup {
			h.restoreLastEntry()
		}
	} else {
		log.Printf("daemon already running (reopen); skipping cold-start recovery and restore")
	}
	action := resolveWindowAction(spawned, settings.lightweight())
	if action == windowEnterBackgroundOnly && h.heldShow() {
		// The user launched the app again while it was starting: they want the window, not background-only running.
		action = windowShow
	}
	switch action {
	case windowEnterBackgroundOnly:
		log.Printf("lightweight cold start: daemon ready, entering lightweight mode")
		h.enterLightweightMode()
	case windowShow:
		log.Printf("lightweight reopen: daemon already running, showing the main window")
		h.showMainWindow()
	}
}

func (h *HostService) ensureDeviceName(settings startupSettings) {
	if settings.DeviceName != "" {
		return
	}
	name, err := os.Hostname()
	if err != nil || name == "" {
		name = "Uniclipboard Device"
	}
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	patch := map[string]any{"general": map[string]any{"deviceName": name}}
	if err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: patch}, nil); err != nil {
		log.Printf("failed to initialize the default device name: %v", err)
	}
}

// recoverAfterColdLaunch unlocks the encryption session from the keyring and, only once that worked,
// advances the daemon's deferred-service lifecycle (those services need the key).
func (h *HostService) recoverAfterColdLaunch() {
	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	var unlocked struct {
		Success *bool `json:"success"`
	}
	err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/encryption/unlock"}, &unlocked)
	switch {
	case err != nil:
		log.Printf("daemon auto-unlock failed; the user will need to enter the passphrase: %v", err)
		return
	case unlocked.Success != nil && !*unlocked.Success:
		log.Printf("encryption not initialized or keyring miss; skip auto-unlock")
		return
	}
	log.Printf("encryption auto-unlocked via daemon")
	if err := h.daemon().Empty(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/lifecycle/retry"}); err != nil {
		log.Printf("daemon lifecycle retry failed: %v", err)
		return
	}
	log.Printf("daemon lifecycle boot completed")
}

// waitForEncryptionSession polls until the session is ready; false when it is not initialized or on timeout.
func (h *HostService) waitForEncryptionSession(timeout time.Duration) bool {
	deadline := time.Now().Add(timeout)
	for {
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		var state struct {
			Initialized  bool `json:"initialized"`
			SessionReady bool `json:"sessionReady"`
		}
		err := h.daemon().Get(ctx, "/encryption/state", &state)
		cancel()
		switch {
		case err == nil && state.SessionReady:
			return true
		case err == nil && !state.Initialized:
			log.Printf("encryption store not initialized; skipping startup restore wait")
			return false
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(200 * time.Millisecond)
	}
}

// restoreLastEntry puts the most recent history entry back on the OS clipboard after preserving what is
// on it now. Both steps touch encrypted history, so the session must be unlocked first; on timeout the
// restore is skipped rather than fired into a locked session.
func (h *HostService) restoreLastEntry() {
	if !h.waitForEncryptionSession(30 * time.Second) {
		log.Printf("encryption session not ready in time; skipping startup restore of the most recent clipboard entry")
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	var page struct {
		Items []struct {
			EntryID string `json:"entryId"`
		} `json:"items"`
	}
	query := url.Values{"query": {""}, "limit": {"1"}, "offset": {"0"}}
	if err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodGet, Path: "/search/query", Query: query}, &page); err != nil {
		log.Printf("failed to list clipboard entries for startup restore: %v", err)
		return
	}
	if len(page.Items) == 0 {
		log.Printf("no clipboard history entry to restore")
		return
	}
	if err := h.daemon().Empty(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/clipboard/capture-current"}); err != nil {
		log.Printf("failed to preserve the current clipboard content before startup restore: %v", err)
	}
	seg, err := daemonclient.PathSegment(page.Items[0].EntryID)
	if err == nil {
		err = h.daemon().Empty(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/clipboard/restore/" + seg})
	}
	if err != nil {
		log.Printf("failed to restore the most recent clipboard entry: %v", err)
		return
	}
	log.Printf("restored the most recent clipboard entry to the OS clipboard")
}
