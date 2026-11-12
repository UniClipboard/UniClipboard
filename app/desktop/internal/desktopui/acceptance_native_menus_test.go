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

// Failure cases: a painted dropdown bypasses the system menu backend; opening
// waits for release; cancelling changes the filter; a native choice loses its
// value or command on the return frame. Exercise the complete history view.
// The tester records the platform menu request; it does not render NSMenu.
func scenarioNativeMenus(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
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
			if err := os.WriteFile(filepath.Join(dir, "native-menus.json"), append(b, '\n'), 0644); err != nil {
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
				// Mouse-down must enter native menu tracking immediately.
				info, ok := tt.AccessByRole(ui.RolePopUpButton, tc.trigger)
				if !ok {
					info, ok = tt.AccessByRole(ui.RoleMenuButton, tc.trigger)
				}
				if !ok {
					t.Fatal("menu trigger missing")
				}
				x, y := info.Bounds.X+info.Bounds.W/2, info.Bounds.Y+info.Bounds.H/2
				tt.Press(x, y)
				items := tt.Menu()
				if !slices.Contains(items, tc.choice) {
					t.Fatalf("system menu did not open on press: %v", items)
				}
				if _, ok := tt.AccessByRole(ui.RoleMenu, ""); ok {
					t.Fatal("menu is still painted in the view")
				}
				tt.CloseMenu()
				tt.Release(x, y)
				if tc.value(d) != before {
					t.Fatal("cancellation changed application state")
				}
				if err := tt.Click(tc.trigger); err != nil {
					t.Fatal(err)
				}
				if err := tt.ChooseMenuItem(tc.choice); err != nil {
					t.Fatal(err)
				}
				if got := tc.value(d); got != tc.want {
					t.Fatalf("choice returned %q, want %q", got, tc.want)
				}
				if tt.Menu() != nil {
					t.Fatal("system menu did not close")
				}
				results = append(results, receipt{theme, tc.trigger, tc.choice, items})
			})
		}
	}
}
