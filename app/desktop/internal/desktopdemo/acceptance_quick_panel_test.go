package desktopdemo

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure inventory (written before implementation):
// - the compact panel inherits the desktop's title bar, filters or detail pane;
// - search steals navigation, accepts a suggestion implicitly or applies stale results;
// - Escape skips the suggestion/filter/close layers; empty Backspace edits plain text;
// - selection wraps, pointer hover changes keyboard selection, or preview races selection;
// - menu replaces history, loses input focus, skips no disabled rows or closes on a page back;
// - paste modifiers, numbered rows and copy-with-selected-text run the wrong action;
// - image navigation uses list geometry or numbering ignores the visible grid page;
// - lost payloads paste, deletion removes the wrong row, or sending targets the local device;
// - lock, empty and disconnected views expose unavailable actions;
// - fixtures call real clipboard, host, network or persistence APIs.
// These scenarios drive the complete mygo views. They do not launch a native window.
func scenarioQuickPanel(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
	receipt := map[string]any{"reference": quickPanelReference, "native_window_acceptance": "NOT RUN", "scenarios": []string{}}
	record := func(s string) { receipt["scenarios"] = append(receipt["scenarios"].([]string), s) }
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			receipt["passed"] = !t.Failed()
			b, err := json.MarshalIndent(receipt, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "quick-panel-e2e.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	create := func() (*quickDemo, *ui.Tester) {
		p := newQuickDemo("dark", "history")
		return p, ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{Dark: true})
	}
	click := func(tt *ui.Tester, name string) {
		t.Helper()
		if err := tt.Click(name); err != nil {
			t.Fatal(err)
		}
	}
	t.Run("geometry-and-preview", func(t *testing.T) {
		p, tt := create()
		receipt["initial_accessibility"] = tt.AccessAll()
		search, ok := tt.AccessByRole(ui.RoleTextField, "搜索快捷面板")
		if !ok {
			t.Fatal("search missing")
		}
		if search.Bounds.Y >= 48 || search.Bounds.H > 48 {
			t.Fatal("header geometry differs")
		}
		row, ok := tt.AccessByRole(ui.RoleButton, "历史：周一产品讨论")
		if !ok || row.Bounds.H != 30 {
			t.Fatalf("row geometry: %+v", row)
		}
		if p.previewID != "" {
			t.Fatal("preview opened before 500 ms")
		}
		tt.Advance(510 * time.Millisecond)
		if p.previewID != p.rows[0].id {
			t.Fatal("first preview missing")
		}
		tt.Key(0, ui.KeyDown)
		if p.selected != 1 {
			t.Fatal("down did not move selection")
		}
		tt.Advance(125 * time.Millisecond)
		if p.previewID != p.rows[1].id {
			t.Fatal("preview did not follow selection")
		}
		tt.Key(0, ui.KeyUp)
		tt.Key(0, ui.KeyUp)
		if p.selected != 0 {
			t.Fatal("list selection wrapped")
		}
		record("geometry-and-preview")
	})
	t.Run("search-suggestions-and-escape", func(t *testing.T) {
		p, tt := create()
		tt.Type("/图片")
		if p.kind != "" {
			t.Fatal("typing accepted suggestion")
		}
		tt.Key(0, ui.KeyTab)
		if p.kind != "图片" || p.query != "" {
			t.Fatal("Tab did not accept image suggestion")
		}
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) < 10 {
			t.Fatal("grid pagination fixture missing")
		}
		tt.Key(0, ui.KeyRight)
		tt.Key(0, ui.KeyDown)
		if p.selected != 4 {
			t.Fatalf("grid selected %d", p.selected)
		}
		tt.Key(0, ui.KeyEscape)
		if p.kind != "" || !p.visible {
			t.Fatal("Escape did not clear filters first")
		}
		tt.Key(0, ui.KeyEscape)
		if p.visible {
			t.Fatal("empty Escape did not close")
		}
		record("search-suggestions-and-escape")
	})
	t.Run("literal-search-and-backspace", func(t *testing.T) {
		p, tt := create()
		tt.Type("#工作")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		if len(p.chips) != 1 {
			t.Fatal("tag not accepted")
		}
		tt.Key(0, ui.KeyBackspace)
		tt.Advance(310 * time.Millisecond)
		if len(p.chips) != 0 {
			t.Fatal("empty Backspace did not remove chip")
		}
		tt.Type("does-not-exist")
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) != 0 {
			t.Fatal("literal search not applied")
		}
		if _, ok := tt.AccessByRole(ui.RoleHeading, "没有匹配“does-not-exist”的内容"); !ok {
			t.Fatal("empty search view missing")
		}
		tt.Key(0, ui.KeyEscape)
		tt.Advance(310 * time.Millisecond)
		if p.query != "" || len(p.rows) == 0 || !p.visible {
			t.Fatal("Escape did not reset search")
		}
		record("literal-search-and-backspace")
	})
	t.Run("actions-and-device-page", func(t *testing.T) {
		p, tt := create()
		tt.Key(ui.Cmd, ui.KeyK)
		if p.menu != "main" {
			t.Fatal("menu missing")
		}
		satellite := ui.NewTesterWith(p.satellite, 380, 400, ui.TesterOptions{Dark: true})
		proof.Capture(t, satellite)
		receipt["actions_accessibility"] = satellite.AccessAll()
		click(satellite, "发送到设备")
		if p.menu != "devices" {
			t.Fatal("device page missing")
		}
		satellite.Frame()
		offline, ok := satellite.AccessByRole(ui.RoleButton, "Linux 工作站")
		if !ok || !offline.Disabled {
			t.Fatal("offline device not disabled")
		}
		tt.Key(0, ui.KeyEscape)
		if p.menu != "main" {
			t.Fatal("Escape closed entire menu from device page")
		}
		tt.Key(0, ui.KeyEscape)
		if p.menu != "" || !p.visible {
			t.Fatal("Escape closed history")
		}
		tt.Key(ui.Cmd, ui.KeyK)
		satellite.Frame()
		click(satellite, "收藏")
		if !p.rows[p.selected].favorite {
			t.Fatal("favorite not updated")
		}
		record("actions-and-device-page")
	})
	t.Run("restore-modifiers-and-numbering", func(t *testing.T) {
		for _, item := range []struct {
			mods        ui.Modifiers
			plain, keep bool
		}{{0, false, false}, {ui.Shift, true, false}, {ui.Cmd, false, true}} {
			p, tt := create()
			tt.Key(item.mods, ui.KeyEnter)
			if len(p.effects) != 1 || p.effects[0].Plain != item.plain || p.effects[0].KeepOpen != item.keep || p.visible != item.keep {
				t.Fatalf("restore mismatch: %+v", p.effects)
			}
		}
		p, tt := create()
		id := p.rows[8].id
		tt.Key(ui.Cmd, ui.Key9)
		if len(p.effects) != 1 || p.effects[0].Entry != id {
			t.Fatal("numbered restore mismatch")
		}
		receipt["sample_effects"] = p.effects
		record("restore-modifiers-and-numbering")
	})
	t.Run("lost-delete-and-hover", func(t *testing.T) {
		p, tt := create()
		tt.Type("丢失的文件")
		tt.Advance(310 * time.Millisecond)
		tt.Key(0, ui.KeyEnter)
		if len(p.effects) != 0 || p.message == "" {
			t.Fatal("lost payload restored")
		}
		tt.Key(ui.Cmd|ui.Shift, ui.KeyBackspace)
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) != 0 {
			t.Fatal("delete failed")
		}
		p, tt = create()
		row, _ := tt.AccessByRole(ui.RoleButton, "历史：HTTP 服务示例")
		tt.Move(row.Bounds.X+20, row.Bounds.Y+15)
		tt.Advance(510 * time.Millisecond)
		if p.selected != 0 || p.previewID != "code" {
			t.Fatalf("hover changed keyboard selection or did not preview: %d %s", p.selected, p.previewID)
		}
		tt.Key(0, ui.KeyDown)
		tt.Advance(125 * time.Millisecond)
		if p.selected != 1 || p.previewID != "code" {
			t.Fatal("keyboard did not retake preview")
		}
		record("lost-delete-and-hover")
	})
	t.Run("grid-scroll-and-satellite-image", func(t *testing.T) {
		p, tt := create()
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Scroll(180, 150, 0, 30)
		if p.gridTop != 1 {
			t.Fatalf("grid wheel page: %d", p.gridTop)
		}
		tt.Key(ui.Cmd, ui.Key1)
		if len(p.effects) != 1 || p.effects[0].Entry != p.rows[3].id {
			t.Fatal("grid number ignored visible page")
		}
		p, tt = create()
		tt.Type("/图片")
		tt.Key(0, ui.KeyTab)
		tt.Advance(850 * time.Millisecond)
		satellite := ui.NewTesterWith(p.satellite, 400, 300, ui.TesterOptions{Dark: true})
		proof.Capture(t, satellite)
		satellite.Move(200, 150)
		satellite.Advance(150 * time.Millisecond)
		click(satellite, "原始尺寸")
		if !p.actualSize {
			t.Fatal("image zoom did not toggle")
		}
		click(satellite, "完整显示")
		if p.actualSize {
			t.Fatal("image fit did not restore")
		}
		record("grid-scroll-and-satellite-image")
	})
	t.Run("multiple-types-and-relaxation", func(t *testing.T) {
		p, tt := create()
		tt.Type("/文本")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Type("/文件")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		hasText, hasFile := false, false
		for _, e := range p.rows {
			hasText = hasText || e.kind == "文本"
			hasFile = hasFile || e.kind == "文件"
		}
		if !hasText || !hasFile {
			t.Fatal("types were combined with AND")
		}
		tt.Key(0, ui.KeyEscape)
		tt.Advance(310 * time.Millisecond)
		tt.Type("/图片")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		tt.Type("#工作")
		tt.Key(0, ui.KeyTab)
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) != 0 || len(p.relaxations()) == 0 {
			t.Fatal("relaxation proposals missing")
		}
		tt.Key(0, ui.KeyEnter)
		tt.Advance(310 * time.Millisecond)
		if len(p.rows) == 0 {
			t.Fatal("Enter did not apply relaxation")
		}
		record("multiple-types-and-relaxation")
	})
	t.Run("copy-query-selection", func(t *testing.T) {
		p, tt := create()
		tt.Type("周一")
		tt.Advance(310 * time.Millisecond)
		tt.Key(ui.Cmd, ui.KeyA)
		tt.Key(ui.Cmd, ui.KeyC)
		if len(p.effects) != 0 || tt.Clipboard() != "周一" {
			t.Fatal("copying selected query restored an entry or lost selection")
		}
		record("copy-query-selection")
	})
	t.Run("themes-and-platform-modifiers", func(t *testing.T) {
		for _, skin := range []string{"macos", "windows", "linux"} {
			t.Setenv("UNIC_UI_SKIN", skin)
			for _, theme := range []string{"light", "dark"} {
				p := newQuickDemo(theme, "history")
				tt := ui.NewTesterWith(p.panel, 380, 366, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if _, ok := tt.AccessByRole(ui.RoleTextField, "搜索快捷面板"); !ok {
					t.Fatal("search missing")
				}
				command := ui.Cmd
				if skin != "macos" {
					command = ui.Ctrl
				}
				click(tt, "操作")
				tt.Key(0, ui.KeyDown)
				if p.menu != "main" || p.cursor != 1 {
					t.Fatal("footer menu did not retain keyboard navigation")
				}
				tt.Key(0, ui.KeyEscape)
				tt.Key(command, ui.KeyEnter)
				if len(p.effects) != 1 || !p.effects[0].KeepOpen {
					t.Fatalf("platform shortcut mismatch: %s/%s", skin, theme)
				}
			}
		}
		record("themes-and-platform-modifiers")
	})
	t.Run("disconnected-empty", func(t *testing.T) {
		for _, mode := range []string{"disconnected", "empty"} {
			p := newQuickDemo("light", mode)
			tt := ui.NewTester(p.panel, 380, 366)
			proof.Capture(t, tt)
			tt.Key(ui.Cmd, ui.KeyK)
			if p.menu != "" {
				t.Fatal("unavailable menu opened")
			}
			if mode == "disconnected" {
				tt.Key(0, ui.KeyEnter)
				tt.Advance(310 * time.Millisecond)
				if p.mode != "history" || len(p.rows) == 0 {
					t.Fatal("reconnect failed")
				}
			}
			host := newQuickDemo("light", mode)
			hostView := ui.NewTester(host.panel, 380, 366)
			proof.Capture(t, hostView)
			hostView.Key(ui.Cmd|ui.Shift, ui.KeyO)
			if len(host.effects) != 1 || host.effects[0].Action != "main" || host.visible {
				t.Fatal("host command unavailable without a selected entry")
			}

		}
		record("disconnected-empty")
	})
}
