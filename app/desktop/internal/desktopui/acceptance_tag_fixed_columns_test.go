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
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"dark", "light"} {
			t.Run(skin+"/"+theme, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", skin)
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				for _, name := range []string{"名称", "条目", "侧边栏"} {
					for _, action := range []string{"shrink", "grow", "reorder"} {
						before, ok := tt.AccessByRole(ui.RoleColumnHeader, name)
						if !ok {
							t.Fatal("missing heading")
						}
						x, y := before.Bounds.X+before.Bounds.W-2, before.Bounds.Y+before.Bounds.H/2
						dx := float32(-80)
						if action == "grow" {
							dx = 300
						}
						if action == "reorder" {
							x = before.Bounds.X + before.Bounds.W/2
							dx = 200
							if name != "名称" {
								dx = -200
							}
						}
						tt.Press(x, y)
						tt.Move(x+dx, y)
						tt.Release(x+dx, y)
						after, _ := tt.AccessByRole(ui.RoleColumnHeader, name)
						results = append(results, measurement{skin, theme, name, action, before.Bounds, after.Bounds})
						if before.Bounds != after.Bounds {
							t.Fatalf("fixed column %s changed during %s: %+v -> %+v", name, action, before.Bounds, after.Bounds)
						}
					}
				}
				if err := tt.Click("在侧边栏显示 #工作"); err != nil {
					t.Fatal(err)
				}
				if d.tagByID("工作").sidebar {
					t.Fatal("switch stopped working")
				}
				if err := tt.RightClick("#工作"); err != nil {
					t.Fatal(err)
				}
				if err := tt.ChooseMenuItem("显示条目"); err != nil {
					t.Fatal(err)
				}
				if d.tagsOpen || d.tag != "工作" {
					t.Fatal("row menu stopped working")
				}
			})
		}
	}
}
