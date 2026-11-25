package desktopui

import (
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
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
			if platform == "darwin" && !system {
				if opts.TrafficLightPosition == nil || *opts.TrafficLightPosition != (mygo.Point{X: 14, Y: 18}) {
					t.Fatal("native traffic lights must fit the application title bar")
				}
			}
			if opts.Frameless {
				t.Error("native resize edges removed")
			}
		}
	}
	// Only macOS integrates native traffic lights into the application top band.
	if runtime.GOOS == "darwin" {
		// v1's shipped macOS SplitView owns one 52-DIP top band. It must not
		// overlap real traffic lights or introduce a second window title row.
		for _, theme := range []string{"light", "dark"} {
			for _, page := range []string{"history", "settings"} {
				t.Run("mac-integrated/"+theme+"/"+page, func(t *testing.T) {
					d := newDemo(page, theme)
					tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
					tt.SetTitleBar(ui.TitleBar{Height: 52, Left: 80})
					proof.Capture(t, tt)
					label, first, title := "切换侧栏", "全部项目", "全部"
					if page == "settings" {
						label, first, title = "切换设置侧栏", "通用", "通用"
					}
					bar, ok := tt.AccessByRole(ui.RoleToolbar, title)
					if !ok || bar.Bounds.Y != 0 || bar.Bounds.H != 52 {
						t.Fatalf("content must start at the window top with one 52-DIP band: %+v", bar.Bounds)
					}
					item, ok := tt.AccessByRole(ui.RoleListItem, first)
					if !ok || item.Bounds.Y < 52 {
						t.Fatalf("sidebar navigation overlaps the native traffic-light band: %+v", item.Bounds)
					}
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
					tt.Advance(350 * time.Millisecond)
					toggle, ok := tt.AccessByRole(ui.RoleButton, label)
					if !ok || toggle.Bounds.X < 88 || toggle.Bounds.Y < 0 || toggle.Bounds.Y+toggle.Bounds.H > 52 {
						t.Fatalf("collapsed sidebar toggle overlaps native controls: %+v", toggle.Bounds)
					}
					tt.SetSize(600, 600)
					toggle, ok = tt.AccessByRole(ui.RoleButton, label)
					if !ok || toggle.Bounds.X < 88 {
						t.Fatalf("narrow toggle overlaps native controls: %+v", toggle.Bounds)
					}
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
					tt.Advance(350 * time.Millisecond)
					if d.sidebarHidden {
						t.Fatal("native title bar blocked the sidebar drawer")
					}
					tt.Key(0, ui.KeyEscape)
					tt.Advance(350 * time.Millisecond)
					if !d.sidebarHidden {
						t.Fatal("sidebar drawer did not close")
					}
				})
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
							var controls []ui.AccessInfo
							for _, node := range tt.AccessAll() {
								if node.Role != ui.RoleSearch && node.Name != "显示方式" && node.Name != "切换收藏" && node.Name != "更多操作" && node.Name != "发送到设备" && node.Name != "动作" && node.Name != "收起搜索" {
									continue
								}
								r := node.Bounds
								if r.Y >= bar.Bounds.Y+bar.Bounds.H {
									continue
								}
								if r.W <= 0 || r.X < 0 || r.X+r.W > caption.Bounds.X+.1 {
									t.Errorf("history control overlaps caption: %s %+v caption=%+v", node.Name, r, caption.Bounds)
								}
								controls = append(controls, node)
							}
							for i := 0; i < len(controls); i++ {
								for j := i + 1; j < len(controls); j++ {
									a, b := controls[i], controls[j]
									if a.Bounds.X < b.Bounds.X+b.Bounds.W-.1 && b.Bounds.X < a.Bounds.X+a.Bounds.W-.1 {
										t.Errorf("history controls overlap: %v(%q)=%+v vs %v(%q)=%+v", a.Role, a.Name, a.Bounds, b.Role, b.Name, b.Bounds)
									}
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
		for _, width := range []int{640, 780} {
			t.Run(fmt.Sprintf("setup/%s/%d", stage, width), func(t *testing.T) {
				closed, minimized := 0, 0
				fields := setupui.Fields{}
				model := setupui.Model{Stage: stage, Error: "Synthetic migration failure"}
				model.WindowControls = func(c *ui.Context) {
					windowchrome.Controls(c, windowchrome.Actions{Close: func() { closed++ }, Minimize: func() { minimized++ }}, "zh-CN")
				}
				tt := ui.NewTesterWith(func(c *ui.Context) {
					setupui.Render(c, model, &fields, setupui.Actions{})
				}, width, 560, ui.TesterOptions{Dark: width == 640})
				proof.Capture(t, tt)
				img := tt.Image()
				if width == 780 && img.At(16, 20) == img.At(520, 20) {
					t.Error("setup panels do not extend to the window top")
				}
				for _, label := range []string{"最小化", "关闭"} {
					button, ok := tt.AccessByRole(ui.RoleButton, label)
					if !ok || button.Bounds.X < float32(width)-132 || button.Bounds.Y+button.Bounds.H > 40 {
						t.Fatalf("setup caption button outside the top-right band: %s %+v", label, button.Bounds)
					}
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
					tt.Frame()
				}
				if closed != 1 || minimized != 1 {
					t.Fatal("setup caption controls did not dispatch")
				}
			})
		}
	}

}
