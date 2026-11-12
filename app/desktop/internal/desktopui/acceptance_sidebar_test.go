package desktopui

import (
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: a padding-width plateau before the sidebar disappears; a
// displaced toolbar after closing; lost navigation after reopening; a stale
// animation target after reversing direction. Drive the complete demo view,
// including real Kit controls and the UI event/layout engine. No native window
// is opened. Geometry checks do not establish visual/native acceptance.
func scenarioSidebarCollapse(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
	d := newDemo("history", "dark")
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
	proof.Capture(t, tt)
	type sample struct {
		Stage    string  `json:"stage"`
		TimeMS   int     `json:"time_ms"`
		ContentX float32 `json:"content_x"`
		ToggleX  float32 `json:"toggle_x"`
	}
	var samples []sample
	read := func(stage string, ms int) sample {
		t.Helper()
		pane, ok := tt.AccessByRole(ui.RoleRegion, "历史列表")
		if !ok {
			t.Fatal("history pane missing")
		}
		toggle, ok := tt.AccessByRole(ui.RoleButton, "切换侧栏")
		if !ok {
			t.Fatal("sidebar toggle missing")
		}
		s := sample{stage, ms, pane.Bounds.X, toggle.Bounds.X}
		samples = append(samples, s)
		return s
	}
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(samples, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "sidebar-collapse-geometry.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	click := func() {
		t.Helper()
		if err := tt.Click("切换侧栏"); err != nil {
			t.Fatal(err)
		}
	}
	if s := read("open", 0); s.ContentX != 236 {
		t.Fatalf("initial sidebar width: %v", s)
	}
	click()
	for _, ms := range []int{40, 80, 120, 160, 200, 240, 260, 280} {
		previous := 0
		if len(samples) > 1 {
			previous = samples[len(samples)-1].TimeMS
		}
		tt.Advance(time.Duration(ms-previous) * time.Millisecond)
		s := read("closing", ms)
		// At 240 ms of a 280 ms ease-out, the pane must already be within
		// four pixels of the edge, rather than waiting at a 20 px floor.
		if ms == 240 && s.ContentX > 4 {
			t.Errorf("sidebar still occupies %.2f px near the end of closing", s.ContentX)
		}
	}
	if s := samples[len(samples)-1]; s.ContentX > .5 {
		t.Errorf("sidebar not closed: %v", s)
	}
	click()
	tt.Advance(280 * time.Millisecond)
	if s := read("reopened", 280); math.Abs(float64(s.ContentX-236)) > .5 {
		t.Errorf("sidebar did not reopen: %v", s)
	}
	click()
	tt.Advance(80 * time.Millisecond)
	read("reverse-start", 80)
	click()
	tt.Advance(280 * time.Millisecond)
	if s := read("reversed-open", 280); math.Abs(float64(s.ContentX-236)) > .5 {
		t.Errorf("reverse animation lost its target: %v", s)
	}
}
