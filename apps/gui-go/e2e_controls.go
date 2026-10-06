//go:build e2e

package main

import (
	"context"
	"math"
	"os"
	"strings"
	"time"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// controlQuickPanel handles the e2e controls for the quick panel, tray and login item. Expectations are computed here
// from the Screen API and the warped cursor, independently of panelOrigin.
func (s *EvidenceService) controlQuickPanel(action string) (bool, error) {
	h := s.host
	switch {
	case strings.HasPrefix(action, "panel-warp:"):
		primary := h.app.Screen.GetPrimary().Bounds
		var x, y float64
		switch strings.TrimPrefix(action, "panel-warp:") {
		case "center":
			x, y = float64(primary.X+primary.Width/2), float64(primary.Y+primary.Height/2)
		case "near":
			x, y = float64(primary.X+300), float64(primary.Y+300)
		case "corner":
			x, y = float64(primary.X+primary.Width-100), float64(primary.Y+primary.Height-100)
		}
		injectCursor(x, y) // quiet mode never moves the real pointer
		return true, nil
	case action == "wait-main-visible":
		// The helper's show_main_window request must bring the hidden main window back.
		w, ok := h.app.Window.GetByName("main")
		visible := false
		for deadline := time.Now().Add(20 * time.Second); ok && time.Now().Before(deadline) && !visible; time.Sleep(100 * time.Millisecond) {
			visible = w.IsVisible()
		}
		return true, s.write(Step{Window: "main", Step: "helper-show-main", OK: visible})
	case action == "prefs":
		// Reads the persisted quick-panel preferences, a cheap probe of whether a settings change stuck.
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		prefs, err := h.loadQuickPanelSettings(ctx)
		return true, s.write(Step{Window: "main", Step: "prefs", OK: err == nil, Detail: prefs})
	case strings.HasPrefix(action, "autostart-state:"):
		// autostart-state:<label>: the stored preference next to the login item on disk.
		label := strings.TrimPrefix(action, "autostart-state:")
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		setting, err := h.autoStartSetting(ctx)
		policy, policyErr := currentLoginItemPolicy()
		// The registration as Wails itself reports it, and the LaunchAgent record it points at when there is one.
		status, statusErr := h.loginItem().Status()
		detail := map[string]any{"setting": setting, "name": policy.name(), "executable": policy.Executable,
			"bundled": runningFromAppBundle(policy.Executable), "bundleID": bundleIdentifier(),
			"enabled": status.Enabled, "strategy": string(status.Strategy), "path": status.Path}
		if statusErr != nil {
			detail["statusError"] = statusErr.Error()
		}
		if status.Strategy == application.AutostartStrategyLaunchAgent {
			if raw, readErr := os.ReadFile(status.Path); readErr == nil {
				detail["plist"] = string(raw)
			}
		}
		return true, s.write(Step{Window: "main", Step: "autostart-" + label, OK: err == nil && policyErr == nil && statusErr == nil, Detail: detail})
	case action == "tray-menu" || strings.HasPrefix(action, "tray-menu:"):
		// The tray menu as the user would read it: labels in order, "-" for separators, submenus nested.
		var walk func(m *application.Menu) []any
		walk = func(m *application.Menu) []any {
			var out []any
			for i := 0; ; i++ {
				item := m.ItemAt(i)
				if item == nil {
					return out
				}
				switch {
				case item.IsSeparator():
					out = append(out, "-")
				case item.IsSubmenu():
					out = append(out, map[string]any{"label": item.Label(), "items": walk(item.GetSubmenu())})
				default:
					out = append(out, map[string]any{"label": item.Label(), "enabled": item.Enabled(), "checked": item.Checked()})
				}
			}
		}
		label := strings.TrimPrefix(strings.TrimPrefix(action, "tray-menu"), ":")
		return true, s.write(Step{Window: "tray", Step: "tray-menu-" + label, OK: h.tray != nil && h.tray.menu != nil, Detail: walk(h.tray.menu)})
	case strings.HasPrefix(action, "tray-devices-wait:"):
		name := strings.TrimPrefix(action, "tray-devices-wait:")
		var detail map[string]any
		for deadline := time.Now().Add(90 * time.Second); time.Now().Before(deadline) && detail == nil; time.Sleep(300 * time.Millisecond) {
			detail = h.tray.devices.itemState(name)
		}
		return true, s.write(Step{Window: "tray", Step: "tray-device-listed", OK: detail != nil, Detail: detail})
	case strings.HasPrefix(action, "tray-device-click:"):
		name := strings.TrimPrefix(action, "tray-device-click:")
		d := h.tray.devices
		id, ok := d.idByName(name)
		if !ok {
			return true, s.write(Step{Window: "tray", Step: "tray-device-toggled", OK: false, Detail: "device not in the menu"})
		}
		d.click(id) // exactly what the menu item's click handler runs
		for deadline := time.Now().Add(20 * time.Second); time.Now().Before(deadline); time.Sleep(200 * time.Millisecond) {
			d.mu.Lock()
			busy := d.pending[id]
			d.mu.Unlock()
			if !busy {
				break
			}
		}
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		prefs, err := h.memberSyncPreferences(ctx, id)
		detail := d.itemState(name)
		detail["send"], detail["receive"] = prefs.SendEnabled, prefs.ReceiveEnabled
		return true, s.write(Step{Window: "tray", Step: "tray-device-toggled", OK: err == nil && detail != nil, Detail: detail})
	case action == "tray-lightweight":
		// The tray's lightweight item: notify, then exit leaving the daemon running.
		go h.enterLightweightMode()
		return true, nil
	case action == "panel-hide":
		h.dismissQuickPanel()
		time.Sleep(300 * time.Millisecond)
		return true, nil
	case strings.HasPrefix(action, "panel-show:"):
		// panel-show:<label>: open the panel for the current settings and report where it landed.
		label := strings.TrimPrefix(action, "panel-show:")
		w, ok := h.app.Window.GetByName(quickPanelWindowName)
		if !ok {
			return true, s.write(Step{Window: quickPanelWindowName, Step: "panel-" + label, OK: false, Detail: "panel window absent"})
		}
		h.showQuickPanel()
		visible := false
		for deadline := time.Now().Add(4 * time.Second); time.Now().Before(deadline) && !visible; time.Sleep(100 * time.Millisecond) {
			visible = w.IsVisible()
		}
		x, y := placedPosition(w)
		width, height := w.Size()
		cx, cy, _ := pointerPosition()
		primary := h.app.Screen.GetPrimary().Bounds
		ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		prefs, _ := h.loadQuickPanelSettings(ctx)
		detail := map[string]any{"visible": visible, "x": x, "y": y, "width": width, "height": height,
			"cursor": []float64{cx, cy}, "primary": primary, "prefs": prefs}
		ok = visible
		switch label {
		case "center":
			wantX := float64(primary.X) + (float64(primary.Width)-float64(width))/2
			wantY := float64(primary.Y) + (float64(primary.Height)-float64(height))/2
			ok = ok && math.Abs(float64(x)-wantX) <= 2 && math.Abs(float64(y)-wantY) <= 2
		case "follow-near":
			ok = ok && math.Abs(float64(x)-(cx+cursorAnchorGap)) <= 2 && math.Abs(float64(y)-(cy+cursorAnchorGap)) <= 2
		case "follow-flipped":
			ok = ok && math.Abs(float64(x)-(cx-cursorAnchorGap-float64(width))) <= 2 && math.Abs(float64(y)-(cy-cursorAnchorGap-float64(height))) <= 2
		case "disabled":
			ok = !visible
		}
		return true, s.write(Step{Window: quickPanelWindowName, Step: "panel-" + label, OK: ok, Detail: detail})
	}
	return false, nil
}
