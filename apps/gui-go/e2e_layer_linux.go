//go:build e2e && linux

package main

import (
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/layershell"
	"github.com/wailsapp/wails/v3/pkg/application"
)

// layerStateDetail is the e2e probe of the Layer Shell panel: what GTK and libgtk-layer-shell report, next to the last
// placement the host computed. It is a claim by the process under test; the compositor-side evidence (surface role,
// namespaces, pixels) is collected independently by the driver.
func layerStateDetail(h *HostService) map[string]any {
	w, ok := h.app.Window.GetByName(quickPanelWindowName)
	if !ok {
		return map[string]any{"panel": false}
	}
	detail := map[string]any{"panel": true, "active": layerPanelActive()}
	application.InvokeSync(func() {
		detail["state"] = layershell.ReadState(w.NativeWindow())
		supported, err := layershell.Supported()
		detail["supported"] = supported
		if err != nil {
			detail["supportedError"] = err.Error()
		}
	})
	layerPanel.Lock()
	detail["placed"] = layerPanel.placed
	if p := layerPanel.placement; p != nil {
		detail["placement"] = map[string]any{"monitor": p.monitor, "x": p.x, "y": p.y, "w": p.w, "h": p.h, "cursor": []float64{p.cursorX, p.cursorY}, "haveCursor": p.haveCursor}
	}
	layerPanel.Unlock()
	return detail
}

// layerNegativeProbe tries to turn the already realized main window into a layer surface: it must be refused.
func layerNegativeProbe(h *HostService) (bool, map[string]any) {
	w, ok := h.app.Window.GetByName("main")
	if !ok {
		return false, map[string]any{"main": false}
	}
	detail := map[string]any{}
	refused := false
	application.InvokeSync(func() {
		handle := w.NativeWindow()
		detail["realized"] = layershell.Realized(handle)
		err := layershell.Attach(handle)
		if err != nil {
			detail["error"] = err.Error()
		}
		refused = detail["realized"] == true && err != nil
	})
	return refused, detail
}
