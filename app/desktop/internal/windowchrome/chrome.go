// Package windowchrome owns main-window decoration and caption actions.
package windowchrome

import (
	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/i18n"
	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

// Configure preserves native resize edges while selecting the title-bar policy.
// The preference is captured once for each main-window creation.
func Configure(opts *mygo.WindowOptions, platform string, system bool) {
	opts.TitleBarStyle = mygo.TitleBarHidden
	opts.HideWindowButtons = platform == "windows" && !system
	if system {
		opts.TitleBarStyle = mygo.TitleBarDefault
	}
}

// Actions use the window's existing close handler and OS maximize state.
type Actions struct {
	Minimize, ToggleMaximize, Close func()
	IsMaximized                     func() bool
}

// NativeView reserves the controls reported by GTK when the title bar is hidden.
func NativeView(c *ui.Context, body func()) {
	k := uikit.For(c.Theme().Dark)
	bar := c.TitleBar()
	k.UiRoot(c, func() {
		ui.Column(c).Fill().Gap(0).AlignItems(ui.Stretch).Children(func() {
			ui.Row(c.Key("native-caption")).Height(max(bar.Height, 32)).Padding(0, bar.Right, 0, bar.Left).DragWindow().Children(func() {
				k.Text(c, "unic", uikit.TextProps{})
			})
			ui.Column(c.Key("window-body")).Grow(1).MinHeight(0).AlignItems(ui.Stretch).Children(body)
		})
	})
}

// Controls supplies the trailing slot of an existing application toolbar.
func Controls(c *ui.Context, actions Actions, language string) {
	k := uikit.For(c.Theme().Dark)
	tr := func(s string) string { return i18n.Translate(language, s) }
	ui.Row(c.Key("window-controls")).Gap(0).Shrink(0).AlignItems(ui.Center).Children(func() {
		button := func(id, label string, icon uikit.IconName, action func()) ui.Element {
			b := k.Button(c.Key(id), "", uikit.ButtonProps{Variant: uikit.Subtle, Size: uikit.SizeSmall, Label: tr(label), Tooltip: tr(label), Icon: icon}).Width(46).Height(32).Shrink(0)
			if b.Clicked() && action != nil {
				action()
			}
			return b
		}
		button("minimize", "最小化", uikit.IconRemove, actions.Minimize)
		label, icon := "最大化", uikit.IconMaximize
		if actions.IsMaximized != nil && actions.IsMaximized() {
			label, icon = "还原", uikit.IconCopy
		}
		button("maximize", label, icon, actions.ToggleMaximize).MaximizeButton()
		button("close", "关闭", uikit.IconDismiss, actions.Close)
	})
}

// View supplies a minimal fallback only for pages without an application toolbar.
func View(c *ui.Context, actions Actions, language string, body func()) {
	k := uikit.For(c.Theme().Dark)
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
