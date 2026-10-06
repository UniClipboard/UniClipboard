//go:build e2e

package main

import (
	"context"
	"math"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// controlQuickPanel drives the quick-panel placement checks. Expectations are computed here
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
		warpCursor(x, y)
		time.Sleep(200 * time.Millisecond)
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
		item, itemErr := loginItem()
		home, _ := os.UserHomeDir()
		raw, readErr := os.ReadFile(filepath.Join(home, "Library", "LaunchAgents", item.Name+".plist"))
		return true, s.write(Step{Window: "main", Step: "autostart-" + label, OK: err == nil && itemErr == nil,
			Detail: map[string]any{"setting": setting, "registered": readErr == nil, "plist": string(raw), "name": item.Name, "executable": item.Executable}})
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
		x, y := w.Position()
		width, height := w.Size()
		cx, cy, _ := cursorPosition()
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
