package desktopui

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure inventory is specified before implementation in the tag-color-parity plan.
// Exercise the complete history view, shared registry, Kit popover and Sheet menu.
func scenarioTagEditor(t *testing.T) {
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"light", "dark"} {
			t.Run(skin+"/"+theme, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", skin)
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				mustClick := func(name string) {
					t.Helper()
					if err := tt.Click(name); err != nil {
						t.Fatal(err)
					}
				}
				original := d.selected
				originalCount := len(d.entries)
				mustClick("添加标签")
				tt.Advance(100 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleComboBox, "标签名"); !ok {
					t.Fatal("inline tag ComboField missing")
				}
				if _, ok := tt.AccessByRole(ui.RoleDialog, "添加标签"); ok {
					t.Fatal("assignment still opens checkbox Sheet")
				}
				tt.Type("旅行")
				if _, ok := tt.AccessByRole(ui.RoleRadioGroup, "颜色"); !ok {
					t.Fatal("create row lacks color group")
				}
				for _, color := range []string{"orange", "blue", "green", "purple", "gray"} {
					if _, ok := tt.AccessByRole(ui.RoleRadio, color); !ok {
						t.Fatal("missing swatch", color)
					}
				}
				mustClick("purple")
				if _, ok := tt.AccessByRole(ui.RoleComboBox, "标签名"); !ok {
					t.Fatal("swatch stole editor")
				}
				if d.tagByID("旅行") != nil {
					t.Fatal("color choice created a tag")
				}
				tt.Key(0, ui.KeyEnter)
				if !slices.Contains(d.current().tags, "旅行") || d.tagColor("旅行") != "purple" {
					t.Fatal("create-and-attach lost name/color")
				}
				if d.selected != original || len(d.entries) != originalCount {
					t.Fatal("tag action changed history or selection")
				}
				mustClick("移除 #旅行")
				if slices.Contains(d.current().tags, "旅行") || d.tagByID("旅行") == nil {
					t.Fatal("remove deleted registry tag or kept association")
				}
				mustClick("添加标签")
				tt.Type("旅行")
				if _, ok := tt.AccessByRole(ui.RoleRadioGroup, "颜色"); ok {
					t.Fatal("existing tag offered duplicate creation")
				}
				tt.Key(0, ui.KeyEnter)
				if !slices.Contains(d.current().tags, "旅行") {
					t.Fatal("existing tag not attached")
				}
				mustClick("添加标签")
				tt.Type("键盘")
				tt.Key(0, ui.KeyTab)
				tt.Key(0, ui.KeyEnter)
				if d.tagColor("键盘") != "blue" {
					t.Fatal("Tab did not cycle creation color")
				}
				mustClick("添加标签")
				tt.Type("取消")
				tt.Key(ui.Shift, ui.KeyTab)
				if d.tagEditor.color != "gray" {
					t.Fatal("Shift-Tab did not wrap colors")
				}
				tt.Key(0, ui.KeyEscape)
				if d.tagByID("取消") != nil {
					t.Fatal("Escape created tag")
				}
				mustClick("添加标签")
				tt.Type("不应创建")
				tt.ClickAt(1090, 710)
				tt.Advance(100 * time.Millisecond)
				if d.tagEditor.open || d.tagByID("不应创建") != nil {
					t.Fatal("click-away did not cancel the editor")
				}
				mustClick("添加标签")
				tt.Type("另一条目")
				mustClick("HTTP 服务示例")
				if d.tagEditor.open || d.tagByID("另一条目") != nil {
					t.Fatal("selection change retained tag intent")
				}
				mustClick("管理标签")
				tt.Advance(350 * time.Millisecond)
				if err := tt.RightClick("#旅行"); err != nil {
					t.Fatal(err)
				}
				if err := tt.ChooseMenuItem("绿色"); err != nil {
					t.Fatal(err)
				}
				if d.tagColor("旅行") != "green" {
					t.Fatal("Sheet color did not reach shared registry")
				}
				mustClick("完成")
				tt.Advance(250 * time.Millisecond)
				mustClick("添加标签")
				tt.Type("截图标签")
				mustClick("blue")
				if _, ok := tt.AccessByRole(ui.RoleOption, "新建 #截图标签"); !ok {
					t.Fatal("create row label missing")
				}
			})
		}
	}
}
