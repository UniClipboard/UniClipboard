package desktopui

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

func scenarioTagManagerPlatforms(t *testing.T) {
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
				table, ok := tt.AccessByRole(ui.RoleTable, "标签")
				if !ok {
					t.Fatal("management table missing")
				}
				// Links is the last row. Taller skins clip it until the table scrolls.
				r := table.Bounds
				tt.Scroll(r.X+r.W/2, r.Y+r.H/2, 0, r.H)
				if err := tt.RightClick("#链接"); err != nil {
					t.Fatal(err)
				}
				if !slices.Equal(tt.Menu(), []string{"显示条目"}) {
					t.Fatal("builtin operations are not protected")
				}
				tt.CloseMenu()
				if err := tt.Click("完成"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(250 * time.Millisecond)
				if err := tt.Click("添加标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleComboBox, "标签名"); !ok || d.tagsOpen {
					t.Fatal("entry assignment did not open inline ComboField")
				}
				tt.Type("自定义标签")
				tt.Key(0, ui.KeyEnter)
				if d.tagCount("custom:0") != 1 || len(d.entries) != 22 {
					t.Fatal("entry assignment did not share the tag registry")
				}
			})
		}
	}
}
