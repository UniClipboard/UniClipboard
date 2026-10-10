package main

import (
	"context"
	"log"
	"os"
	"runtime"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/quickpanelhelper"
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
// modifier double-tap trigger and the window; otherwise the WebView panel is pre-created. A
// missing helper executable falls back to the WebView panel so the user is never left without one.
func (h *HostService) initQuickPanel() {
	if nativePanelWanted(os.Getenv(nativePanelEnv), nativePanelDefault()) {
		if exe, ok := helperExecutable(); ok {
			log.Printf("using the native quick panel helper: %s", exe)
			h.helper = quickpanelhelper.Start(quickpanelhelper.ForHelper(exe, h.handleHelperRequest))
			ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
			enabled := true // the setting's default when it cannot be read
			if prefs, err := h.loadQuickPanelSettings(ctx); err == nil {
				enabled = prefs.Enabled
			}
			cancel()
			h.helper.SetEnabled(enabled)
			return
		}
		log.Printf("the native quick panel is selected but the helper executable was not found; using the WebView quick panel")
	}
	h.preCreateQuickPanel()
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

// panelEnabledChanged applies the persisted `quickPanel.enabled` to the running implementation.
func (h *HostService) panelEnabledChanged(enabled bool) {
	if h.helper != nil {
		h.helper.SetEnabled(enabled)
	} else if !enabled {
		h.dismissQuickPanel()
	}
}

// restartPanelHelper makes a running helper re-read settings it only applies at startup.
func (h *HostService) restartPanelHelper() {
	if h.helper != nil {
		h.helper.Restart()
	}
}
