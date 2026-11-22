package desktopui

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
// bypass the in-window menu; builtin tags expose destructive operations; edits
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

				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleList, "标签"); !ok {
					t.Fatal("tag management list missing")
				}
				menu := func(name string) []string {
					t.Helper()
					if err := boarduiTableClick(tt, "标签", name, 0, true); err != nil {
						t.Fatal(err)
					}
					items := boarduiMenuItems(tt)
					if len(items) == 0 {
						t.Fatal("no accessible in-window context menu shown")
					}
					if len(boarduiMenuItems(tt)) == 0 {
						t.Fatal("in-window context popup has no accessible items")
					}
					return items
				}
				choose := func(path ...string) {
					t.Helper()
					if err := boarduiChooseMenu(tt, path...); err != nil {
						t.Fatal(err)
					}
				}
				var items []string
				var heights []float32
				initialTagCount := len(d.tags)
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
					if _, ok := tt.AccessByRole(ui.RoleListItem, "工作"); ok {
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
					list, _ := tt.AccessByRole(ui.RoleList, "标签")
					designShown, selfShown := false, false
					for _, a := range tt.AccessAll() {
						if a.Bounds.X < list.Bounds.X || a.Bounds.Y < list.Bounds.Y || a.Bounds.Y >= list.Bounds.Y+list.Bounds.H {
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
						t.Fatal("tag filter did not narrow list rows")
					}
				case "protected-delete":
					if err := boarduiTableClick(tt, "标签", "#链接", 0, false); err != nil {
						t.Fatal(err)
					}
					if a, ok := tt.AccessByRole(ui.RoleButton, "删除选中标签"); ok && !a.Disabled {
						t.Fatal("builtin selection enables delete")
					}
					if items = menu("#链接"); !slices.Equal(items, []string{"显示条目"}) {
						t.Fatalf("builtin context menu exposed destructive actions: %v", items)
					}
				case "geometry":
					list, _ := tt.AccessByRole(ui.RoleList, "标签")
					rows := 0
					for _, a := range tt.AccessAll() {
						if a.Role != ui.RoleListItem || a.Offscreen || a.Bounds.X < list.Bounds.X-.1 || a.Bounds.Y < list.Bounds.Y || a.Bounds.Y >= list.Bounds.Y+list.Bounds.H {
							continue
						}
						rows++
						if a.Bounds.X+a.Bounds.W > list.Bounds.X+list.Bounds.W+.1 || a.Bounds.H <= 0 || a.Bounds.H > 42 {
							t.Errorf("tag row geometry out of bounds: row=%+v list=%+v", a.Bounds, list.Bounds)
						}
						heights = append(heights, a.Bounds.H)
					}
					if rows < 2 {
						t.Fatal("tag row geometry missing")
					}
				case "cancel":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("临时标签")
					tt.Key(0, ui.KeyEscape)
					if !d.tagsOpen || d.tagManager.editing != "" || len(d.tags) != initialTagCount {
						t.Fatal("Escape did not cancel inline creation")
					}
				case "duplicate":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("工作")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("已存在同名标签") || len(d.tags) != initialTagCount {
						t.Fatal("duplicate tag was accepted")
					}
					tt.Key(ui.Cmd, ui.KeyA)
					tt.Type("外出")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("#外出") || len(d.tags) != initialTagCount+1 {
						t.Fatal("duplicate error could not be corrected")
					}
				case "bulk":
					if err := boarduiTableClick(tt, "标签", "#工作", 0, false); err != nil {
						t.Fatal(err)
					}
					if err := boarduiTableClick(tt, "标签", "#设计", ui.Cmd, false); err != nil {
						t.Fatal(err)
					}
					items = menu("#工作")
					choose("删除 2 个标签（保留条目）")
					if len(d.entries) != 22 || d.tagByID("工作") != nil || d.tagByID("设计") != nil || d.tagByID("builtin:link") == nil {
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
					list, ok := tt.AccessByRole(ui.RoleList, "标签")
					if !ok {
						t.Fatal("tag list missing after sort")
					}
					tt.Scroll(list.Bounds.X+list.Bounds.W/2, list.Bounds.Y+list.Bounds.H/2, 0, -10000)
					first := ""
					y := float32(10000)
					for _, item := range tt.AccessAll() {
						if item.Role != ui.RoleListItem || item.Offscreen || !strings.HasPrefix(item.Name, "#") || item.Bounds.Y >= y {
							continue
						}
						if item.Bounds.X < list.Bounds.X-.1 || item.Bounds.X+item.Bounds.W > list.Bounds.X+list.Bounds.W+.1 || item.Bounds.Y < list.Bounds.Y || item.Bounds.Y >= list.Bounds.Y+list.Bounds.H {
							continue
						}
						y, first = item.Bounds.Y, item.Name
					}
					if !strings.HasPrefix(first, "#自定义标签") {
						t.Fatalf("latest tag not first logical list row after scrolling to top: %q", first)
					}

				}
				results = append(results, receipt{theme, scenario, items, len(d.entries), heights})
			})
		}
	}
}
