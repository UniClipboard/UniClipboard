//go:build windows

package main

import (
	"encoding/json"
	"log"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
)

// probeHelperShortcuts refuses a shortcut that another program already holds before it is saved. The native helper
// registers its own global shortcut and only logs a registration failure (a conflict must not end the process the
// host supervises), so without this the settings page would accept a binding that never fires. Windows grants a
// hot key to one owner, so the probe registers each new binding through the host's own binder and releases it again
// at once; bindings the helper already holds are skipped, since their registration would fail for that reason.
func (h *HostService) probeHelperShortcuts(current, next map[string]json.RawMessage, enabled bool) error {
	if !enabled || !shortcutBackendAllowed() {
		return nil
	}
	held := resolveQuickPanelShortcuts(current)
	var fresh []string
	for _, shortcut := range resolveQuickPanelShortcuts(next) {
		if !contains(held, shortcut) {
			fresh = append(fresh, shortcut)
		}
	}
	if len(fresh) == 0 {
		return nil
	}
	binder := h.shortcutBinder()
	if err := updateShortcuts(binder, nil, fresh); err != nil {
		return hostapi.New(hostapi.CodeConflict, err.Error())
	}
	for _, shortcut := range fresh {
		if err := binder.unregister(shortcut); err != nil {
			log.Printf("failed to release the probed global shortcut %q: %v", shortcut, err)
		}
	}
	return nil
}
