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
				if _, ok := tt.AccessByRole(ui.RoleTable, "标签"); !ok {
					t.Fatal("management table missing")
				}
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
				if !d.assignTagsOpen || d.tagsOpen {
					t.Fatal("entry assignment opened management instead")
				}
				if err := tt.Click("#自定义标签"); err != nil {
					t.Fatal(err)
				}
				if err := tt.Click("完成"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(250 * time.Millisecond)
				if d.tagCount("custom:0") != 1 || len(d.entries) != 22 {
					t.Fatal("entry assignment did not share the tag registry")
				}
			})
		}
	}
}
