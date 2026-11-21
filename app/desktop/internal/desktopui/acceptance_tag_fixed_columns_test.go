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

// Failure cases: dragging a divider resizes a column, dragging a heading
// reorders columns, or fixing columns prevents row and switch interaction.
func scenarioTagFixedColumns(t *testing.T) {
	type measurement struct {
		Skin, Theme, Column, Action string
		Before, After               ui.Rect
	}
	var results []measurement
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
			if err := os.WriteFile(filepath.Join(dir, "tag-fixed-columns.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, skin := range []string{"boardui"} {
		for _, theme := range []string{"dark", "light"} {
			t.Run(skin+"/"+theme, func(t *testing.T) {

				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				list, ok := tt.AccessByRole(ui.RoleList, "标签")
				if !ok {
					t.Fatal("missing tag list")
				}
				// Grouped rows keep uniform alignment and width inside the list viewport.
				var workBefore ui.Rect
				for _, a := range tt.AccessAll() {
					if a.Role == ui.RoleListItem && a.Name == "#工作" && a.Bounds.X >= list.Bounds.X-.1 {
						workBefore = a.Bounds
						break
					}
				}
				if workBefore.W <= 0 || workBefore.H != 40 {
					t.Fatalf("unexpected grouped tag row bounds: %+v", workBefore)
				}
				results = append(results, measurement{skin, theme, "row", "initial", workBefore, workBefore})
				if err := tt.Click("在侧边栏显示 #工作"); err != nil {
					t.Fatal(err)
				}
				if d.tagByID("工作").sidebar {
					t.Fatal("switch stopped working")
				}
				if err := boarduiTableClick(tt, "标签", "#工作", 0, true); err != nil {
					t.Fatal(err)
				}
				if err := boarduiChooseMenu(tt, "显示条目"); err != nil {
					t.Fatal(err)
				}
				if d.tagsOpen || d.tag != "工作" {
					t.Fatal("row menu stopped working")
				}
			})
		}
	}
}
