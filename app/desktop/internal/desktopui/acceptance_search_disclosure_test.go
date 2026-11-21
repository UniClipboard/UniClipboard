package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Verifies the U1 centered unified search + filter omnibar: always visible in
// the toolbar without moving the trailing command group, Cmd+F focuses the
// input, clear retains focus, Escape closes suggestions then clears the query,
// and the integrated filter funnel button toggles the chip popover.
func scenarioSearchDisclosure(t *testing.T) {

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
	initialW := read()
	if initialW < 180 || initialW > 460 {
		t.Errorf("initial unified omnibar width=%v, want within [180, 460]", initialW)
	}
	pin, _ := tt.AccessByRole(ui.RoleButton, "切换置顶")
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	if d.search.open {
		t.Error("clicking empty unified search box must not open legacy suggestion panel")
	}
	if w := read(); w != initialW {
		t.Errorf("focusing unified omnibar shifted width: got %v want %v", w, initialW)
	}
	after, _ := tt.AccessByRole(ui.RoleButton, "切换置顶")
	if after.Bounds != pin.Bounds {
		t.Error("search moved the command group")
	}
	tt.Type("HTTP 服务示例")
	if d.query != "HTTP 服务示例" || len(d.filtered()) != 1 {
		t.Errorf("search input/results: %q %d", d.query, len(d.filtered()))
	}
	if d.search.open {
		t.Error("plain text search must not open legacy suggestion panel")
	}
	if err := tt.Click("全部项目"); err != nil {
		t.Fatal(err)
	}
	if w := read(); w != initialW {
		t.Error("unified omnibar width changed on focus loss")
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
	if d.query != "" {
		t.Error("Escape on plain text query without popup did not clear the query")
	}
	// Integrated filter popover opens via the trailing funnel button.
	if err := tt.Click("筛选条件"); err != nil {
		t.Fatal(err)
	}
	if !d.filterOpen {
		t.Error("filter funnel button did not open filter popover")
	}
	chip, ok := tt.AccessByRole(ui.RoleButton, "代码")
	if !ok {
		t.Fatal("filter chip button 代码 missing in popover")
	}
	tt.ClickAt(chip.Bounds.X+chip.Bounds.W/2, chip.Bounds.Y+chip.Bounds.H/2)
	if d.kind != "代码" {
		t.Errorf("filter chip did not apply kind=代码, got %q", d.kind)
	}
	// Cmd+F closes the filter popover and focuses the input.
	tt.Key(ui.Cmd, ui.KeyF)
	if d.filterOpen {
		t.Error("Cmd+F did not dismiss open filter popover")
	}
	if active, _ := tt.TextInputActive(); !active {
		t.Error("Cmd+F did not focus unified omnibar input")
	}
}

func scenarioSearchDisclosureReducedMotion(t *testing.T) {

	d := newDemo("history", "light")
	tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true, TextScale: 1}})
	proof.Capture(t, tt)
	if err := tt.Click("搜索历史"); err != nil {
		t.Fatal(err)
	}
	info, ok := tt.AccessByRole(ui.RoleSearch, "")
	if !ok || info.Bounds.W < 180 {
		t.Fatalf("Reduce Motion unified omnibar: %+v", info.Bounds)
	}
	tt.Type("HTTP")
	tt.Key(0, ui.KeyEscape)
	tt.Key(0, ui.KeyEscape)
	if d.query != "" {
		t.Fatalf("Reduce Motion Escape did not clear query: %q", d.query)
	}
}
