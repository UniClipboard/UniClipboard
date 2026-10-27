package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"runtime"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/globalshortcut"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/settings"
)

// nativePanelEnv selects the native quick panel (`1`) or the WebView one (`0`); macOS and Windows
// default to native, as in the Tauri shell. Linux keeps the WebView panel (layer-shell and X11 placement
// live in the WebView window code).
const nativePanelEnv = "UC_GPUI_QUICK_PANEL"

// nativePanelDefault is whether the native panel is used when the environment does not choose. Without the
// helper executable next to the app the WebView panel is used anyway.
func nativePanelDefault() bool {
	return runtime.GOOS == "darwin" || runtime.GOOS == "windows"
}

func nativePanelWanted(value string, defaultOn bool) bool {
	switch value {
	case "1":
		return true
	case "0":
		return false
	}
	return defaultOn
}

// initQuickPanel picks the quick panel implementation once at startup, so no code path has to ask
// whether the other one is running too. The native helper takes over the global shortcut, the
// modifier double-tap trigger and the window; otherwise the host dispatches to its UI child. A
// missing helper executable falls back to the WebView panel so the user is never left without one.
// Either way the host keeps the shortcut's desired and active binding (ADR-013 section 10).
// Lightweight hosts construct the same controllers but leave their resources disabled.
func (h *HostService) initQuickPanel() {
	if nativePanelWanted(os.Getenv(nativePanelEnv), nativePanelDefault()) {
		if exe, ok := helperExecutable(); ok {
			log.Printf("using the native quick panel helper: %s", exe)
			h.helper = quickpanelhelper.Start(quickpanelhelper.ForHelper(exe, h.handleHelperRequest))
			h.panelShortcut = globalshortcut.New(helperShortcuts{h.helper}, h.shortcutStatusChanged)
			// A launched helper holds no shortcut until it is told one.
			h.helper.OnLaunch(h.panelShortcut.Relaunched)
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			enabled := true // the setting's default when it cannot be read
			view, err := settings.Read(ctx, h.daemon())
			if err == nil {
				enabled = view.QuickPanel.Enabled
			}
			cancel()
			enabled = h.panelEnabled(enabled)
			h.shortcutsMu.Lock()
			h.appliedPanel = appliedPanelSettings{known: err == nil, enabled: enabled, modifier: view.QuickPanel.DoubleTapModifier}
			h.setHelperEnabled(enabled, panelShortcutTarget(enabled, view.KeyboardShortcuts))
			h.shortcutsMu.Unlock()
			return
		}
		log.Printf("the native quick panel is selected but the helper executable was not found; using the WebView quick panel")
	}
	h.panelShortcut = globalshortcut.New(webViewShortcuts{h}, h.shortcutStatusChanged)
	h.preCreateQuickPanel()
}

// helperShortcuts is the native helper's executor for globalshortcut.Applier.
type helperShortcuts struct{ helper *quickpanelhelper.Supervisor }

func (e helperShortcuts) Apply(bindings []string) ([]string, error) {
	held, err := e.helper.SetShortcuts(bindings)
	if errors.Is(err, quickpanelhelper.ErrNoAnswer) {
		return nil, fmt.Errorf("%w: %v", globalshortcut.ErrUnknown, err)
	}
	return held, err
}

// setHelperEnabled starts or stops the helper with the panel switch. The desired binding is recorded first: a launch
// applies it (Relaunched), a stop leaves nothing bound. The caller holds shortcutsMu.
func (h *HostService) setHelperEnabled(enabled bool, target []string) {
	h.panelShortcut.Record(target)
	h.helper.SetEnabled(enabled)
	if !enabled {
		h.panelShortcut.Stopped()
	}
}

// shortcutStatusChanged forwards the quick panel shortcut's status to the pages.
func (h *HostService) shortcutStatusChanged(status globalshortcut.Status) {
	h.emit(shortcutStatusEvent, toShortcutStatus(status))
}

// handleHelperRequest carries out what the helper asks of the host.
func (h *HostService) handleHelperRequest(request quickpanelhelper.Request) {
	switch request {
	case quickpanelhelper.ShowMainWindow:
		h.showMainWindow()
	case quickpanelhelper.OpenSettings:
		h.showSettings()
	}
}
