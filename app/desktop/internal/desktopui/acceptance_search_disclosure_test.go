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

// Failure cases: an always-expanded search field; an instantaneous width jump;
// a moved title/command group; lost input focus; an empty field that cannot
// collapse. Drive the complete existing toolbar without a native replacement.
func scenarioSearchDisclosure(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
	d := newDemo("history", "dark")
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: true})
	proof.Capture(t, tt)
	widths := []float32{}
	read := func() float32 {
		a, ok := tt.AccessByRole(ui.RoleSearch, "")
		if !ok {
			t.Fatal("search missing")
		}
		widths = append(widths, a.Bounds.W)
		return a.Bounds.W
	}
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, _ := json.MarshalIndent(widths, "", "  ")
			if err := os.WriteFile(filepath.Join(dir, "search-disclosure.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	if w := read(); w != 32 {
		t.Errorf("initial search width=%v, want icon width 32", w)
	}
	pin, _ := tt.AccessByRole(ui.RoleButton, "切换置顶")
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(40 * time.Millisecond)
	if w := read(); w <= 32 || w >= 460 {
		t.Errorf("no intermediate expansion width: %v", w)
	}
	tt.Advance(300 * time.Millisecond)
	if w := read(); w != 460 {
		t.Errorf("expanded search width=%v", w)
	}
	after, _ := tt.AccessByRole(ui.RoleButton, "切换置顶")
	if after.Bounds != pin.Bounds {
		t.Error("search moved the command group")
	}
	tt.Type("HTTP 服务示例")
	if d.query != "HTTP 服务示例" || len(d.filtered()) != 1 {
		t.Errorf("search input/results: %q %d", d.query, len(d.filtered()))
	}
	if err := tt.Click("全部项目"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(300 * time.Millisecond)
	if w := read(); w != 460 {
		t.Error("nonempty search collapsed on focus loss")
	}
	tt.Key(ui.Cmd, ui.KeyF)
	if err := tt.Click("清除搜索"); err != nil {
		t.Fatal(err)
	}
	if d.query != "" || len(d.filtered()) != 22 {
		t.Error("clear did not restore all results")
	}
	tt.Type("HTTP 服务示例")
	if d.query != "HTTP 服务示例" {
		t.Error("clear did not retain input focus")
	}
	tt.Key(0, ui.KeyEscape)
	if d.query != "HTTP 服务示例" {
		t.Error("first Escape must preserve text while closing suggestions")
	}
	tt.Key(0, ui.KeyEscape)
	if d.query != "" {
		t.Error("second Escape did not clear the query")
	}
	tt.Key(0, ui.KeyEscape)
	tt.Advance(40 * time.Millisecond)
	if w := read(); w <= 32 || w >= 460 {
		t.Errorf("no intermediate collapse width: %v", w)
	}
	tt.Advance(300 * time.Millisecond)
	if w := read(); w != 32 {
		t.Errorf("search did not collapse: %v", w)
	}
	tt.Key(ui.Cmd, ui.KeyF)
	tt.Advance(300 * time.Millisecond)
	if w := read(); w != 460 {
		t.Error("Cmd+F did not expand search")
	}
	tt.Key(0, ui.KeyEscape)
	tt.Key(0, ui.KeyEscape)
	tt.Advance(40 * time.Millisecond)
	tt.Key(ui.Cmd, ui.KeyF)
	tt.Advance(320 * time.Millisecond)
	if w := read(); w != 460 {
		t.Error("search did not settle after reversing a collapse")
	}
	if err := tt.Click("全部项目"); err != nil {
		t.Fatal(err)
	}
	tt.Advance(300 * time.Millisecond)
	if w := read(); w != 32 {
		t.Error("empty search did not collapse on focus loss")
	}
}

func scenarioSearchDisclosureReducedMotion(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
	d := newDemo("history", "light")
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, TextScale: 1}})
	proof.Capture(t, tt)
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	info, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok || info.Bounds.W != 460 {
		t.Fatalf("Reduce Motion expansion: %+v", info.Bounds)
	}
	tt.Key(0, ui.KeyEscape)
	tt.Key(0, ui.KeyEscape)
	info, _ = tt.AccessByRole(ui.RoleSearch, "")
	if info.Bounds.W != 32 {
		t.Fatalf("Reduce Motion collapse: %+v", info.Bounds)
	}
}
