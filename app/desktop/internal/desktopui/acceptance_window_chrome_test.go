package desktopui

import (
	"testing"

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
				if opts.TrafficLightPosition == nil || *opts.TrafficLightPosition != (mygo.Point{X: 14, Y: 9}) {
					t.Fatal("native traffic lights must fit the application title bar")
				}
			}
			if opts.Frameless {
				t.Fatal("resize edges must remain available")
			}
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
