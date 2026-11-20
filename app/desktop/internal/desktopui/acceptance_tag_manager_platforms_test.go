package desktopui

import (
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

func scenarioTagManagerPlatforms(t *testing.T) {
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
				table, ok := tt.AccessByRole(ui.RoleTable, "标签")
				if !ok {
					t.Fatal("management table missing")
				}
				// Links is the last row. Scroll the real table before accessing its menu.
				r := table.Bounds
				tt.Scroll(r.X+r.W/2, r.Y+r.H/2, 0, r.H)
				if err := boarduiTableClick(tt, "标签", "#链接", 0, true); err != nil {
					t.Fatal(err)
				}
				if !slices.Equal(boarduiMenuItems(tt), []string{"显示条目"}) {
					t.Fatal("builtin operations are not protected")
				}
				tt.Key(0, ui.KeyEscape)
				if err := tt.Click("完成"); err != nil {
					t.Fatal(err)
				}
				// Source Sheet exit retains the modal scope for 300ms.
				tt.Advance(350 * time.Millisecond)
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
