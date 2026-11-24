// Package windowchrome owns main-window decoration and caption actions.
package windowchrome

import (
	"runtime"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
)

// Configure preserves native resize edges while selecting the title-bar policy.
// The preference is captured once for each main-window creation.
func Configure(opts *mygo.WindowOptions, platform string, system bool) {
	opts.TitleBarStyle = mygo.TitleBarHidden
	opts.HideWindowButtons = platform == "windows" && !system
	if system {
		opts.TitleBarStyle = mygo.TitleBarDefault
	} else if platform == "darwin" {
		opts.TrafficLightPosition = &mygo.Point{X: 14, Y: 18}
	}
}

// Actions use the window's existing close handler and OS maximize state.
type Actions struct {
	Minimize, ToggleMaximize, Close func()
	IsMaximized                     func() bool
}

// NativeView reserves host caption controls when the native title bar is hidden.
func NativeView(c *ui.Context, body func()) {
	k := desktopkit.For(c.Theme().Dark)
	bar := c.TitleBar()
	height, left, right, alignment := max(bar.Height, 38), bar.Left, bar.Right, ui.Start
	if runtime.GOOS == "darwin" {
		// The application paints this bar; macOS paints its real traffic lights.
		height = max(bar.Height, 32)
		left = max(bar.Left, bar.Right) + 12
		right, alignment = left, ui.Center
	}
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Gap(0).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("native-caption")).Height(height).Padding(0, right, 0, left).AlignItems(ui.Center).Justify(alignment).DragWindow().Children(func() {
				k.Text(c, "unic", desktopkit.TextProps{})
			})
			ui.Column(c.Key("window-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(body)
		})
	})
}

// Controls supplies the trailing slot of an existing application toolbar.
func Controls(c *ui.Context, actions Actions, language string) {
	k := desktopkit.For(c.Theme().Dark)
	tr := func(s string) string { return i18n.Translate(language, s) }
	ui.Row(c.Key("window-controls")).Gap(2).Shrink(0).AlignItems(ui.Center).Children(func() {
		button := func(id, label string, icon desktopkit.IconName, action func()) ui.Element {
			name := tr(label)
			b := ui.ButtonBase(c.Key(id)).Size(42, 32).Shrink(0).Radius(5).
				AlignItems(ui.Center).Justify(ui.Center).Label(name).Tooltip(name).
				FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(0)
			ink := k.Color("foreground-icon-secondary")
			pressed := b.Pressed() || (b.Focused() && c.KeyDown(ui.KeySpace))
			if pressed || b.Hovered() {
				ink = k.Color("foreground-icon-primary")
				fill := ink.Alpha(.08)
				if pressed {
					fill = ink.Alpha(.14)
				}
				if id == "close" {
					ink = k.Color("text-on-danger")
					fill = k.Color("gradient-button-danger-default-to")
					if pressed {
						fill = k.Color("gradient-button-danger-active-to")
					}
				}
				b.Background(fill)
			}
			b.Children(func() {
				k.Icon(c.Key("glyph"), icon, 14).TextColor(ink)
			})
			if b.Clicked() && action != nil {
				action()
			}
			return b
		}
		button("minimize", "最小化", desktopkit.IconRemove, actions.Minimize)
		label, icon := "最大化", desktopkit.IconMaximize
		if actions.IsMaximized != nil && actions.IsMaximized() {
			label, icon = "还原", desktopkit.IconCopy
		}
		button("maximize", label, icon, actions.ToggleMaximize).MaximizeButton()
		button("close", "关闭", desktopkit.IconDismiss, actions.Close)
	})
}

// View supplies a minimal fallback only for pages without an application toolbar.
func View(c *ui.Context, actions Actions, language string, body func()) {
	k := desktopkit.For(c.Theme().Dark)
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Gap(0).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("caption")).Height(48).Shrink(0).AlignItems(ui.Center).DragWindow().Children(func() {
				ui.Spacer(c)
				Controls(c, actions, language)
			})
			ui.Column(c.Key("window-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(body)
		})
	})
}
