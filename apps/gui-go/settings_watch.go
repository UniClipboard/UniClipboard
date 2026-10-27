package main

import (
	"context"
	"log"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
)

// startSettingsWatch follows the daemon's settings events for the host's own consumers. It runs from bootstrap, so a
// silent start without any window gets external changes (CLI, another client) as well.
func (h *HostService) startSettingsWatch() {
	ctx, stop := context.WithCancel(context.Background())
	h.stopSettings = stop
	go h.settingsWatch.Run(ctx)
}

// applySettings hands the daemon's current settings to each host consumer. It runs after every read, including the
// read caused by the host's own write, so every consumer compares with the state it holds and does nothing when it
// is unchanged.
func (h *HostService) applySettings(view settings.View) {
	lightweight := view.General.StartupMode == startupLightweight
	// Linux needs its tray configured before app.Run. Only a windowless host
	// without a tray needs replacement; existing desktop resources stay alive.
	if !lightweight && h.tray == nil {
		if h.modeRestarting.CompareAndSwap(false, true) {
			go func() {
				defer h.modeRestarting.Store(false)
				// A replacement reads the latest persisted mode; it never rewrites it.
				if err := h.restartShell(true); err != nil {
					log.Printf("desktop mode restart failed: %v", err)
					h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
				}
			}()
		}
		return
	}
	// Serialize residency changes with UI reaping and the idle-exit decision.
	h.ui.mu.Lock()
	if h.quitting.Load() {
		h.ui.mu.Unlock()
		return
	}
	h.lightweight.Store(lightweight)
	h.ui.mu.Unlock()
	defer h.exitLightweightIfIdle()
	if h.tray == nil {
		return
	}
	h.tray.setSyncEnabled(view.Sync.SyncEnabled)
	// The tray language follows the stored language (B9). Only a change is applied, so a read does not rebuild the menu.
	if language := resolveTrayLanguage(view.General.Language); language != h.trayLanguageApplied {
		h.trayLanguageApplied = language
		h.tray.setLanguage(language)
	}
	h.tray.icon.setStyle(view.General.TrayIcon)
	h.reconcilePanel(view)
}
