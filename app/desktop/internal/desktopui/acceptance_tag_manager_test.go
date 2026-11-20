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
				if _, ok := tt.AccessByRole(ui.RoleTable, "标签"); !ok {
					t.Fatal("tag management table missing")
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
					if err := boarduiTableClick(tt, "标签", "#链接", 0, false); err != nil {
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
						// A virtualized 48-DIP final row can be partly clipped by
						// the 360-DIP viewport. Require intersection and fixed
						// horizontal bounds, not a whole-row scroll alignment.
						rows++
						if a.Bounds.X < table.Bounds.X-.1 || a.Bounds.X+a.Bounds.W > table.Bounds.X+table.Bounds.W+.1 || a.Bounds.Y+a.Bounds.H <= table.Bounds.Y || a.Bounds.H <= 0 {
							t.Errorf("table row has no visible intersection or exceeds the column bounds: row=%+v table=%+v", a.Bounds, table.Bounds)
						}
						heights = append(heights, a.Bounds.H)
						limit := float32(48)
						if a.Bounds.Y == table.Bounds.Y {
							limit = 32
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
					if !d.tagsOpen || d.tagManager.editing != "" || len(d.tags) != 10 {
						t.Fatal("Escape did not cancel inline creation")
					}
				case "duplicate":
					if err := tt.Click("新建标签"); err != nil {
						t.Fatal(err)
					}
					tt.Type("工作")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("已存在同名标签") || len(d.tags) != 10 {
						t.Fatal("duplicate tag was accepted")
					}
					tt.Key(ui.Cmd, ui.KeyA)
					tt.Type("外出")
					tt.Key(0, ui.KeyEnter)
					if !tt.HasText("#外出") || len(d.tags) != 11 {
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
					// Sorting may retain the virtual table's old scroll anchor.
					// Inspect the first logical row after explicitly returning to
					// the top, not whichever old row remained visible on screen.
					table, ok := tt.AccessByRole(ui.RoleTable, "标签")
					if !ok {
						t.Fatal("tag table missing after sort")
					}
					tt.Scroll(table.Bounds.X+table.Bounds.W/2, table.Bounds.Y+table.Bounds.H/2, 0, -10000)
					all := tt.AccessAll()
					first := ""
					y := float32(10000)
					for _, text := range all {
						if text.Role != ui.RoleText || text.Offscreen || !strings.HasPrefix(text.Name, "#") || text.Bounds.Y >= y {
							continue
						}
						x, cy := text.Bounds.X+text.Bounds.W/2, text.Bounds.Y+text.Bounds.H/2
						if x < table.Bounds.X || x >= table.Bounds.X+table.Bounds.W || cy < table.Bounds.Y || cy >= table.Bounds.Y+table.Bounds.H {
							continue
						}
						for _, cell := range all {
							if cell.Role != ui.RoleCell && cell.Role != ui.RoleRowHeader {
								continue
							}
							r := cell.Bounds
							if !cell.Offscreen && r.X >= table.Bounds.X-.1 && r.X+r.W <= table.Bounds.X+table.Bounds.W+.1 && x >= r.X && x < r.X+r.W && cy >= r.Y && cy < r.Y+r.H {
								y, first = text.Bounds.Y, text.Name
								break
							}
						}
					}
					if !strings.HasPrefix(first, "#自定义标签") {
						t.Fatalf("latest tag not first logical table row after scrolling to top: %q", first)
					}

				}
				results = append(results, receipt{theme, scenario, items, len(d.entries), heights})
			})
		}
	}
}
