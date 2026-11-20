package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: opening a popup performs an action; cancellation changes a
// filter; the view delegates to a legacy system menu; a choice loses its value
// or command. Exercise real in-window options in the complete history view.
func scenarioBoardUIMenus(t *testing.T) {

	type receipt struct {
		Theme, Trigger, Choice string
		Items                  []string
	}
	var results []receipt
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
			if err := os.WriteFile(filepath.Join(dir, "boardui-menus.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, tc := range []struct {
			trigger, choice string
			value           func(*viewState) string
			want            string
		}{
			{"内容类型", "代码", func(d *viewState) string { return d.kind }, "代码"},
			{"来源设备", "Linux 工作站", func(d *viewState) string { return d.source }, "linux"},
			{"标签筛选", "#设计", func(d *viewState) string { return d.tag }, "设计"},
			{"时间范围", "今天", func(d *viewState) string { return d.timeRange }, "today"},
			{"发送到设备", "工作电脑", func(d *viewState) string { return d.notice }, "演示：已发送到工作电脑"},
			{"更多操作", "管理标签", func(d *viewState) string {
				if d.tagsOpen {
					return "open"
				}
				return "closed"
			}, "open"},
		} {
			t.Run(theme+"/"+tc.trigger, func(t *testing.T) {
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				before := tc.value(d)
				// A complete pointer click opens the real in-window popup.
				info, ok := tt.AccessByRole(ui.RolePopUpButton, tc.trigger)
				if !ok {
					info, ok = tt.AccessByRole(ui.RoleMenuButton, tc.trigger)
				}
				if !ok {
					t.Fatal("menu trigger missing")
				}
				x, y := info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2
				tt.Press(x, y)
				tt.Release(x, y)
				items := boarduiMenuItems(tt)
				if !slices.Contains(items, tc.choice) {
					t.Fatalf("BoardUI popup did not open: %v", items)
				}
				if len(boarduiMenuItems(tt)) == 0 {
					t.Fatal("in-window popup has no accessible menu/listbox items")
				}
				tt.Key(0, ui.KeyEscape)
				if tc.value(d) != before {
					t.Fatal("cancellation changed application state")
				}
				if err := tt.Click(tc.trigger); err != nil {
					t.Fatal(err)
				}
				if err := boarduiChooseMenu(tt, tc.choice); err != nil {
					t.Fatal(err)
				}
				if got := tc.value(d); got != tc.want {
					t.Fatalf("choice returned %q, want %q", got, tc.want)
				}
				if boarduiMenuItems(tt) != nil {
					t.Fatal("BoardUI menu did not close")
				}
				results = append(results, receipt{theme, tc.trigger, tc.choice, items})
			})
		}
	}
}
