package desktopdemo

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: a checkbox task replaces the management table; context menus
// bypass the native backend; builtin tags expose destructive operations; edits
// drift from search/sidebar data; tag deletion removes clipboard history.
func scenarioTagManager(t *testing.T) {
	type receipt struct {
		Theme, Scenario string
		Menu            []string
		Entries         int
		RowHeights      []float32
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
			if err := os.WriteFile(filepath.Join(dir, "tag-manager.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, scenario := range []string{"builtin", "create", "rename", "color", "sidebar", "delete", "merge", "show", "filter", "protected-delete", "geometry", "cancel", "duplicate", "bulk", "sort"} {
			t.Run(theme+"/"+scenario, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", "macos")
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleTable, "标签"); !ok {
					t.Fatal("tag management table missing")
				}
				menu := func(name string) []string {
					t.Helper()
					if err := tt.RightClick(name); err != nil {
						t.Fatal(err)
					}
					items := tt.Menu()
					if len(items) == 0 {
						t.Fatal("no native context menu requested")
					}
					if _, ok := tt.AccessByRole(ui.RoleMenu, ""); ok {
						t.Fatal("context menu is still painted by the app")
					}
					return items
				}
				choose := func(path ...string) {
					t.Helper()
					if err := tt.ChooseMenuItem(path...); err != nil {
						t.Fatal(err)
					}
				}
				var items []string
				var heights []float32
				switch scenario {
				case "builtin":
					items = menu("#链接")
					if !slices.Equal(items, []string{"显示条目"}) {
						t.Fatalf("builtin menu: %v", items)
					}
				case "create":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("旅行")
					tt.Key(0, ui.KeyEnter)
					items = menu("#旅行")
					if !slices.Contains(items, "重命名…") {
						t.Fatal("new tag is not editable")
					}
				case "rename":
					items = menu("#设计")
					choose("重命名…")
					tt.Key(ui.Cmd, ui.KeyA)
					tt.Type("视觉设计")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("#视觉设计") {
						t.Fatal("rename not shown")
					}
					if err := tt.Click("完成"); err != nil {
						t.Fatal(err)
					}
					if !tt.HasText("#视觉设计") {
						t.Fatal("sidebar did not reflect rename")
					}
				case "color":
					items = menu("#工作")
					choose("紫色")
					if d.tagColor("工作") != "purple" {
						t.Fatal("color action did not update the tag")
					}
					menu("#工作")
					choose("显示条目")
					if d.tag != "工作" || len(d.filtered()) != 3 {
						t.Fatal("recolored tag lost its associations")
					}
				case "sidebar":
					if err := tt.Click("在侧边栏显示 #工作"); err != nil {
						t.Fatal(err)
					}
					if err := tt.Click("完成"); err != nil {
						t.Fatal(err)
					}
					if _, ok := tt.AccessByRole(ui.RoleListItem, "#工作"); ok {
						t.Fatal("hidden tag remains in sidebar")
					}
				case "delete":
					items = menu("#工作")
					choose("删除标签（保留条目）")
					if len(d.entries) != 22 {
						t.Fatal("tag deletion removed history")
					}
					for _, e := range d.entries {
						if slices.Contains(e.tags, "工作") {
							t.Fatal("deleted tag association remains")
						}
					}
					if tt.HasText("#工作") {
						t.Fatal("deleted tag remains in the sheet")
					}
				case "merge":
					items = menu("#工作")
					choose("合并到", "#设计")
					if len(d.entries) != 22 || tt.HasText("#工作") || d.tagCount("设计") != 4 {
						t.Fatal("merge did not preserve history or remove the source")
					}
				case "show":
					items = menu("#链接")
					choose("显示条目")
					if d.tagsOpen || len(d.filtered()) != 1 || d.filtered()[0].id != "link" {
						t.Fatal("builtin show-items filter failed")
					}
				case "filter":
					if err := tt.Click("筛选标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("自定义")
					table, _ := tt.AccessByRole(ui.RoleTable, "标签")
					designShown, selfShown := false, false
					for _, a := range tt.AccessAll() {
						if a.Bounds.X < table.Bounds.X || a.Bounds.Y < table.Bounds.Y || a.Bounds.Y >= table.Bounds.Y+table.Bounds.H {
							continue
						}
						if a.Name == "#设计" {
							designShown = true
						}
						if a.Name == "#自定义标签" {
							selfShown = true
						}
					}
					if !selfShown || designShown {
						t.Fatal("tag filter did not narrow table rows")
					}
				case "protected-delete":
					if err := tt.Click("#链接"); err != nil {
						t.Fatal(err)
					}
					a, ok := tt.AccessByRole(ui.RoleButton, "删除选中标签")
					if !ok || !a.Disabled {
						t.Fatal("builtin selection enables delete")
					}
				case "geometry":
					table, _ := tt.AccessByRole(ui.RoleTable, "标签")
					rows := 0
					for _, a := range tt.AccessAll() {
						if a.Role != ui.RoleRow || a.Bounds.Y < table.Bounds.Y || a.Bounds.Y >= table.Bounds.Y+table.Bounds.H {
							continue
						}
						rows++
						heights = append(heights, a.Bounds.H)
						limit := float32(34)
						if a.Bounds.Y == table.Bounds.Y {
							limit = 28
						}
						if a.Bounds.H > limit {
							t.Errorf("table row height=%v, limit=%v", a.Bounds.H, limit)
						}
					}
					if rows < 2 {
						t.Fatal("table row geometry missing")
					}
				case "cancel":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("临时标签")
					tt.Key(0, ui.KeyEscape)
					if !d.tagsOpen || d.tagManager.editing != "" || len(d.tags) != 8 {
						t.Fatal("Escape did not cancel inline creation")
					}
				case "duplicate":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("工作")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("已存在同名标签") || len(d.tags) != 8 {
						t.Fatal("duplicate tag was accepted")
					}
					tt.Key(ui.Cmd, ui.KeyA)
					tt.Type("外出")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("#外出") || len(d.tags) != 9 {
						t.Fatal("duplicate error could not be corrected")
					}
				case "bulk":
					if err := tt.Click("#工作"); err != nil {
						t.Fatal(err)
					}
					if err := tt.ClickWith(ui.Cmd, "#设计"); err != nil {
						t.Fatal(err)
					}
					items = menu("#工作")
					choose("删除 2 个标签（保留条目）")
					if len(d.entries) != 22 || d.tagByID("工作") != nil || d.tagByID("设计") != nil || d.tagByID("link") == nil {
						t.Fatal("bulk deletion did not preserve builtins/history")
					}
				case "sort":
					if err := tt.Click("标签排序"); err != nil {
						t.Fatal(err)
					}
					choose("最新")
					if d.tagManager.sort != "newest" {
						t.Fatal("sort did not apply")
					}
					first := ""
					y := float32(10000)
					table, _ := tt.AccessByRole(ui.RoleTable, "标签")
					for _, a := range tt.AccessAll() {
						if a.Bounds.X >= table.Bounds.X && a.Bounds.Y >= table.Bounds.Y && a.Bounds.Y < table.Bounds.Y+table.Bounds.H && strings.HasPrefix(a.Name, "#") && a.Bounds.Y < y {
							y = a.Bounds.Y
							first = a.Name
						}
					}
					if !strings.HasPrefix(first, "#自定义标签") {
						t.Fatalf("latest tag not first: %s", first)
					}
				}
				results = append(results, receipt{theme, scenario, items, len(d.entries), heights})
			})
		}
	}
}
