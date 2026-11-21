package desktopui

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/app/desktop/internal/setupui"
	"github.com/uniclipboard/unic/app/desktop/internal/windowchrome"
)

// Full-view E2E. Failure inventory precedes implementation in the execution receipt.
func scenarioWindowChrome(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "windows")
	geometry := map[string][]ui.Rect{}
	t.Cleanup(func() {
		data, err := json.MarshalIndent(geometry, "", "  ")
		if err != nil {
			t.Error(err)
			return
		}
		if err := os.WriteFile(filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"), "toolbar-geometry.json"), data, 0600); err != nil {
			t.Error(err)
		}
	})
	for _, platform := range []string{"linux", "darwin", "windows"} {
		for _, system := range []bool{false, true} {
			opts := mygo.WindowOptions{}
			windowchrome.Configure(&opts, platform, system)
			if system && opts.TitleBarStyle != mygo.TitleBarDefault {
				t.Error("system title bar missing", platform)
			}
			if !system && opts.TitleBarStyle != mygo.TitleBarHidden {
				t.Error("hidden title bar missing", platform)
			}
			if opts.HideWindowButtons != (platform == "windows" && !system) {
				t.Error("incorrect native buttons", platform, system)
			}
			if opts.Frameless {
				t.Error("native resize edges removed")
			}
		}
	}
	for _, width := range []int{600, 640, 1100} {
		for _, dark := range []bool{false, true} {
			for _, system := range []bool{false, true} {
				t.Run(fmt.Sprintf("%d/dark%t/system%t", width, dark, system), func(t *testing.T) {
					theme := "light"
					if dark {
						theme = "dark"
					}
					d := newDemo("history", theme)
					minimized, closed, maximized := 0, 0, false
					actions := windowchrome.Actions{
						Minimize: func() { minimized++ }, Close: func() { closed++ },
						ToggleMaximize: func() { maximized = !maximized }, IsMaximized: func() bool { return maximized },
					}
					tt := ui.NewTesterWith(func(c *ui.Context) {
						if !system {
							d.toolbarTrailing = func(c *ui.Context) { windowchrome.Controls(c, actions, "zh-CN") }
						}
						d.view(c)
					}, width, 720, ui.TesterOptions{Dark: dark})
					if width == 640 {
						tt.SetScale(2)
					}
					checkpoint := func(page string) {
						t.Helper()
						path := filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"), fmt.Sprintf("%s-%x.png", page, sha256.Sum256([]byte(t.Name()))))
						file, err := os.Create(path)
						if err != nil {
							t.Fatal(err)
						}
						encodeErr := png.Encode(file, tt.Image())
						closeErr := file.Close()
						if encodeErr != nil {
							t.Fatal(encodeErr)
						}
						if closeErr != nil {
							t.Fatal(closeErr)
						}
					}
					proof.Capture(t, tt)
					assertBar := func(title string) {
						t.Helper()
						bar, ok := tt.AccessByRole(ui.RoleToolbar, title)
						if !ok {
							t.Fatal("missing complete application toolbar", title)
						}
						geometry[t.Name()+"/"+title] = append(geometry[t.Name()+"/"+title], bar.Bounds)
						if bar.Bounds.Y > 1 {
							t.Errorf("extra title row above toolbar: %+v", bar.Bounds)
						}
						previous := float32(-1)
						for _, label := range []string{"最小化", "最大化", "关闭"} {
							b, ok := tt.AccessByRole(ui.RoleButton, label)
							if system {
								if ok {
									t.Error("duplicate custom caption", label)
								}
								continue
							}
							if !ok {
								t.Error("missing caption", label)
								continue
							}
							r := b.Bounds
							geometry[t.Name()+"/"+title] = append(geometry[t.Name()+"/"+title], r)
							if r.X < previous || r.X+r.W > float32(width)+.1 || r.Y < bar.Bounds.Y || r.Y+r.H > bar.Bounds.Y+bar.Bounds.H+.1 {
								t.Errorf("caption outside shared toolbar: %s %+v bar=%+v", label, r, bar.Bounds)
							}
							previous = r.X + r.W
						}
						if !system && title == "全部" {
							caption, _ := tt.AccessByRole(ui.RoleButton, "最小化")
							for _, node := range tt.AccessAll() {
								if node.Role != ui.RoleSearch && node.Name != "切换置顶" && node.Name != "更多操作" && node.Name != "发送到设备" {
									continue
								}
								r := node.Bounds
								if r.Y >= bar.Bounds.Y+bar.Bounds.H {
									continue
								}
								if r.W <= 0 || r.X < 0 || r.X+r.W > caption.Bounds.X+.1 {
									t.Errorf("history control overlaps caption: %s %+v caption=%+v", node.Name, r, caption.Bounds)
								}
							}
						}
					}
					assertBar("全部")
					checkpoint("history")
					tt.Key(ui.Cmd, ui.KeyF)
					tt.Advance(350 * time.Millisecond)
					assertBar("全部")
					if !system {
						for _, label := range []string{"最小化", "最大化", "还原", "关闭"} {
							if err := tt.Click(label); err != nil {
								t.Fatal(err)
							}
							tt.Frame()
						}
						if minimized != 1 || closed != 1 || maximized {
							t.Error("caption lifecycle actions did not dispatch exactly once")
						}
					}
					d.page = "settings"
					tt.Frame()
					assertBar("通用")
					checkpoint("general")
					d.settings.Value = "plugins"
					tt.Frame()
					assertBar("插件")
				})
			}
		}
	}
	for _, stage := range []string{"welcome", "import", "failed"} {
		t.Run("fallback/"+stage, func(t *testing.T) {
			closed, minimized := 0, 0
			fields := setupui.Fields{}
			tt := ui.NewTesterWith(func(c *ui.Context) {
				windowchrome.View(c, windowchrome.Actions{Close: func() { closed++ }, Minimize: func() { minimized++ }}, "zh-CN", func() {
					setupui.Render(c, setupui.Model{Stage: stage, Error: "Synthetic startup or migration failure"}, &fields, setupui.Actions{})
				})
			}, 640, 560, ui.TesterOptions{})
			proof.Capture(t, tt)
			for _, label := range []string{"最小化", "关闭"} {
				if err := tt.Click(label); err != nil {
					t.Fatal(err)
				}
				tt.Frame()
			}
			if closed != 1 || minimized != 1 {
				t.Fatal("fallback controls did not dispatch")
			}
		})
	}

}
