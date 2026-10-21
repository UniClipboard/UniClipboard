package main

import (
	"context"

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
	h.tray.setSyncEnabled(view.Sync.SyncEnabled)
	h.tray.icon.setStyle(view.General.TrayIcon)
}
