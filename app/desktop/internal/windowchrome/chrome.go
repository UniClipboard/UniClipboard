// Package windowchrome owns main-window decoration and caption actions.
package windowchrome

import (
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
)

// Configure preserves native resize edges while selecting the title-bar policy.
// Windows always draws its own controls; the saved preference is for Linux/macOS.
func Configure(opts *mygo.WindowOptions, platform string, system bool) {
	opts.TitleBarStyle = mygo.TitleBarHidden
	opts.HideWindowButtons = platform == "windows"
	if system && platform != "windows" {
		opts.TitleBarStyle = mygo.TitleBarDefault
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
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Gap(0).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("native-caption")).Height(max(bar.Height, 38)).Padding(0, bar.Right, 0, bar.Left).DragWindow().Children(func() {
				k.Text(c, "unic", desktopkit.TextProps{})
			})
			ui.Column(c.Key("window-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(body)
		})
	})
}

// View draws Windows controls above every main-window page, including setup.
func View(c *ui.Context, actions Actions, language string, body func()) {
	k := desktopkit.For(c.Theme().Dark)
	tr := func(s string) string { return i18n.Translate(language, s) }
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Gap(0).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("caption")).Height(32).Gap(0).AlignItems(ui.Center).Children(func() {
				ui.Row(c.Key("caption-drag")).Grow(1).Height(32).Padding(0, 12).AlignItems(ui.Center).DragWindow().Children(func() {
					k.Text(c, "unic", desktopkit.TextProps{})
				})
				button := func(id, label string, icon desktopkit.IconName, action func()) {
					if k.Button(c.Key(id), "", desktopkit.ButtonProps{Variant: desktopkit.Subtle, Size: desktopkit.SizeSmall, Label: tr(label), Icon: icon}).Width(46).Height(32).Clicked() && action != nil {
						action()
					}
				}
				button("minimize", "最小化", desktopkit.IconRemove, actions.Minimize)
				label, icon := "最大化", desktopkit.IconMaximize
				if actions.IsMaximized != nil && actions.IsMaximized() {
					label, icon = "还原", desktopkit.IconCopy
				}
				button("maximize", label, icon, actions.ToggleMaximize)
				button("close", "关闭", desktopkit.IconDismiss, actions.Close)
			})
			ui.Column(c.Key("window-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(body)
		})
	})
}
