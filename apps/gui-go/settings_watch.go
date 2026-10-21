package main

import (
	"context"
	"log"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// startSettingsWatch follows the daemon's settings events for the host's own consumers. It runs from bootstrap, so a
// silent start without any window gets external changes (CLI, another client) as well.
func (h *HostService) startSettingsWatch() {
	ctx, stop := context.WithCancel(context.Background())
	h.stopSettings = stop
	go h.settingsWatch.Run(ctx)
	// The page still announces its own saves. The host does not use the page's copy of the settings: it reads the
	// daemon, like the watcher, so both triggers apply the same state. Removed together with the page's emit when the
	// frontend follows settings.changed.
	h.app.Event.On(settingsChangedEvent, func(*application.CustomEvent) { go h.readAndApplySettings() })
}

func (h *HostService) readAndApplySettings() {
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	view, err := settings.Read(ctx, h.daemon())
	if err != nil {
		log.Printf("settings read after a page save failed: %v", err)
		return
	}
	h.applySettings(view)
}

// applySettings hands the daemon's current settings to each host consumer. It runs after every read, including the
// read caused by the host's own write, so every consumer compares with the state it holds and does nothing when it
// is unchanged.
func (h *HostService) applySettings(view settings.View) {
	h.tray.setSyncEnabled(view.Sync.SyncEnabled)
}
