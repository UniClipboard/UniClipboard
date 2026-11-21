package desktopui

import (
	"testing"
	"time"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/app/desktop/internal/windowchrome"
)

func scenarioWindowChrome(t *testing.T) {

	for _, platform := range []string{"darwin", "linux", "windows"} {
		for _, system := range []bool{false, true} {
			opts := mygo.WindowOptions{}
			windowchrome.Configure(&opts, platform, system)
			wantNative := system && platform != "windows"
			if wantNative && opts.TitleBarStyle != mygo.TitleBarDefault {
				t.Fatal("system title bar missing", platform)
			}
			if !wantNative && opts.TitleBarStyle != mygo.TitleBarHidden {
				t.Fatal("custom title bar missing", platform)
			}
			if opts.HideWindowButtons != (platform == "windows") {
				t.Fatal("incorrect native buttons", platform)
			}
			if platform == "darwin" && !system {
				if opts.TrafficLightPosition == nil || *opts.TrafficLightPosition != (mygo.Point{X: 14, Y: 18}) {
					t.Fatal("native traffic lights must fit the application title bar")
				}
			}
			if opts.Frameless {
				t.Fatal("resize edges must remain available")
			}
		}
	}
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
	for _, dark := range []bool{false, true} {
		minimized, closed, maximized := 0, 0, false
		actions := windowchrome.Actions{
			Minimize: func() { minimized++ }, Close: func() { closed++ },
			ToggleMaximize: func() { maximized = !maximized }, IsMaximized: func() bool { return maximized },
		}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			desktopkit.For(dark).Apply(c)
			windowchrome.View(c, actions, "zh-CN", func() { ui.Text(c, "Window body") })
		}, 640, 480, ui.TesterOptions{Dark: dark})
		proof.Capture(t, tt)
		for _, label := range []string{"最小化", "最大化", "还原", "关闭"} {
			if err := tt.Click(label); err != nil {
				t.Fatal(err)
			}
			tt.Frame()
		}
		if minimized != 1 || closed != 1 || maximized {
			t.Fatal("caption actions did not dispatch exactly once")
		}
	}
}
