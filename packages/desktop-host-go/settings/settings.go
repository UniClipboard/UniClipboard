// Package settings is the Go hosts' view of the daemon's aggregate `GET /settings` and the
// watcher that keeps it current from the WS topic `settings` (ADR-013 section 7). It has no GUI
// dependency, so a host without any WebView (silent start) receives every change.
package settings

import (
	"context"
	"encoding/json"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// View is the part of `GET /settings` the Go hosts read. The JSON paths follow the daemon's
// SettingsDto; a path that does not match decodes to the zero value without an error, so a new
// field must be checked against a real response.
type View struct {
	General           General                    `json:"general"`
	Sync              Sync                       `json:"sync"`
	KeyboardShortcuts map[string]json.RawMessage `json:"keyboardShortcuts"`
	QuickPanel        QuickPanel                 `json:"quickPanel"`
}

// General is the `general` section.
type General struct {
	AutoStart                 bool    `json:"autoStart"`
	StartupMode               string  `json:"startupMode"`
	RestoreLastEntryOnStartup bool    `json:"restoreLastEntryOnStartup"`
	DeviceName                string  `json:"deviceName"`
	AutoCheckUpdate           bool    `json:"autoCheckUpdate"`
	AutoDownloadUpdate        bool    `json:"autoDownloadUpdate"`
	UpdateChannel             *string `json:"updateChannel"` // nil: no explicit channel
}

// Sync is the `sync` section.
type Sync struct {
	SyncEnabled bool `json:"syncEnabled"`
}

// QuickPanel is the `quickPanel` section.
type QuickPanel struct {
	Enabled           bool   `json:"enabled"`
	Position          string `json:"position"`
	DoubleTapModifier string `json:"doubleTapModifier"`
}

// Read returns the daemon's current settings.
func Read(ctx context.Context, client *daemonclient.Client) (View, error) {
	var view View
	err := client.Get(ctx, "/settings", &view)
	return view, err
}
