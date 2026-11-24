package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: missing source 0.98 press/return; canceled press activating;
// selection losing state after reversal; a legacy system-menu request.
// Exercise the complete view through pointer events without a native window.
func scenarioInteractionMotion(t *testing.T) {

	results := map[string]any{}
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(results, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "interaction-motion.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	t.Run("button", func(t *testing.T) {
		d := newDemo("history", "dark")
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
		proof.Capture(t, tt)
		read := func() ui.Rect {
			a, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
			if !ok {
				t.Fatal("pin missing")
			}
			return a.Bounds
		}
		r := read()
		initialPinned := d.current().pinned
		x, y := r.X+r.W/2, r.Y+r.H/2
		tt.Press(x, y)
		tt.Advance(220 * time.Millisecond)
		pressed := read()
		tt.Release(x, y)
		if d.current().pinned == initialPinned {
			t.Error("release did not activate the button")
		}
		tt.Advance(40 * time.Millisecond)
		returning := read()
		if returning.W <= pressed.W || returning.W >= r.W {
			t.Error("release does not interpolate back to the resting size")
		}
		// Retarget during the return, then cancel outside the control.
		tt.Press(x, y)
		tt.Advance(220 * time.Millisecond)
		tt.Move(0, 0)
		tt.Release(0, 0)
		if d.current().pinned == initialPinned {
			t.Error("cancelled press activated the button")
		}
		tt.Advance(500 * time.Millisecond)
		released := read()
		results["button"] = []ui.Rect{r, pressed, returning, released}
		if ratio := pressed.W / r.W; ratio < .978 || ratio > .982 {
			t.Errorf("button press ratio=%g, want fixed-source 0.98", ratio)
		}
		if released.W < r.W-.1 || released.W > r.W+.1 {
			t.Error("button did not return to its resting size")
		}
	})
	t.Run("selection", func(t *testing.T) {
		d := newDemo("history", "dark")
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
		proof.Capture(t, tt)
		tt.SetScale(2)
		r, _ := tt.AccessByRole(ui.RoleToggleButton, "网格")
		tt.Move(r.Bounds.X+r.Bounds.W/2, r.Bounds.Y+r.Bounds.H/2)
		tt.Advance(200 * time.Millisecond)
		pixel := func() any {
			// Grid mode inserts its size slider before this group. Sample the
			// current group-local point, not the old world coordinate.
			g, ok := tt.AccessByRole(ui.RoleGroup, "显示方式")
			if !ok {
				t.Fatal("segmented group missing")
			}
			return tt.Image().RGBAAt(int(2*(g.Bounds.X+18)), int(2*(g.Bounds.Y+8)))
		}
		before := pixel()
		tt.ClickAt(r.Bounds.X+r.Bounds.W/2, r.Bounds.Y+r.Bounds.H/2)
		first := pixel()
		tt.Advance(350 * time.Millisecond)
		last := pixel()
		results["selection"] = []any{before, first, last}
		if d.layout != "grid" {
			t.Error("selection click did not change the layout")
		}
		if before == last {
			t.Error("selection feedback did not reach the new segment")
		}
		// Reverse an unfinished trip without losing the final selection.
		left, ok := tt.AccessByRole(ui.RoleToggleButton, "列表")
		if !ok {
			t.Fatal("list segment missing")
		}
		tt.ClickAt(left.Bounds.X+left.Bounds.W/2, left.Bounds.Y+left.Bounds.H/2)
		tt.Advance(40 * time.Millisecond)
		tt.ClickAt(r.Bounds.X+r.Bounds.W/2, r.Bounds.Y+r.Bounds.H/2)
		tt.Advance(500 * time.Millisecond)
		if d.layout != "grid" || pixel() != last {
			t.Error("selection did not settle after rapid reversal")
		}
	})
	t.Run("menu", func(t *testing.T) {
		d := newDemo("history", "dark")
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
		proof.Capture(t, tt)
		if err := tt.Click("更多操作"); err != nil {
			t.Fatal(err)
		}
		first := boarduiMenuItems(tt)
		if len(first) == 0 {
			t.Fatal("BoardUI popup missing")
		}
		tt.Advance(180 * time.Millisecond)
		results["menu"] = first
		if len(boarduiMenuItems(tt)) == 0 {
			t.Error("in-window popup has no accessible items")
		}
	})
	t.Run("reduce-motion", func(t *testing.T) {
		d := newDemo("history", "dark")
		tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true, Preferences: ui.Preferences{ReduceMotion: true, TextScale: 1}})
		proof.Capture(t, tt)
		b, ok := tt.AccessByRole(ui.RoleButton, "切换收藏")
		if !ok {
			t.Fatal("pin missing")
		}
		tt.Press(b.Bounds.X+b.Bounds.W/2, b.Bounds.Y+b.Bounds.H/2)
		tt.Advance(220 * time.Millisecond)
		p, _ := tt.AccessByRole(ui.RoleButton, "切换收藏")
		if p.Bounds != b.Bounds {
			t.Error("press scales with Reduce Motion enabled")
		}
		tt.Release(0, 0)
		if err := tt.Click("更多操作"); err != nil {
			t.Fatal(err)
		}
		if len(boarduiMenuItems(tt)) == 0 {
			t.Fatal("BoardUI popup missing")
		}
		results["reduce-motion"] = []ui.Rect{b.Bounds, p.Bounds}
		if len(boarduiMenuItems(tt)) == 0 {
			t.Error("reduced-motion popup has no accessible items")
		}
	})
}
