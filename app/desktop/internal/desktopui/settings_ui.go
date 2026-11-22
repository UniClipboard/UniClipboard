package desktopui

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

// settingsPill renders a compact semantic capsule badge (matching prototype/settings.html .pill).
func settingsPill(c *ui.Context, k desktopkit.Kit, text string, tone desktopkit.StatusTone, dot, mono bool) ui.Element {
	bg, fg, dotCol := k.Color("background-tertiary-default"), k.Color("text-secondary"), k.Color("text-tertiary")
	switch tone {
	case desktopkit.StatusPositive:
		bg, fg = k.Color("status-lime-background"), k.Color("status-lime-text")
		dotCol = k.Color("notification-success-foreground")
	case desktopkit.StatusWarning:
		bg, fg = k.Color("status-yellow-background"), k.Color("status-yellow-text")
		dotCol = k.Color("status-yellow-text")
	case desktopkit.StatusInfo:
		bg, fg = k.Color("status-blue-background"), k.Color("status-blue-text")
		dotCol = k.Color("accent-500")
	case desktopkit.StatusCritical:
		bg, fg = k.Color("background-tertiary-error"), k.Color("text-error-primary")
		dotCol = k.Color("text-error-primary")
	}
	return ui.Row(c).Height(21).Padding(0, 8).Radius(11).AlignItems(ui.Center).Gap(5).Shrink(0).Background(bg).Children(func() {
		if dot {
			ui.Box(c.Key("dot")).Size(6, 6).Radius(3).Shrink(0).Background(dotCol).AccessHidden(true)
		}
		t := ui.Text(c.Key("text"), text).FontSize(11).FontWeight(600).TextColor(fg).SingleLine()
		if mono {
			t.Font("monospace")
		}
	})
}

// settingsStrip renders a calm, padded notice strip inside a SettingsGroup card (.s-strip).
func settingsStrip(c *ui.Context, k desktopkit.Kit, tone desktopkit.NoticeTone, text string) ui.Element {
	bg, fg := k.SubtleCardSurface(), k.Color("text-secondary")
	icon := desktopkit.IconInfo
	switch tone {
	case desktopkit.ToneInfo:
		bg, fg = k.Color("accent-50"), k.Color("accent-600")
		if k.Dark {
			fg = k.Color("accent-400")
		}
	case desktopkit.ToneWarning:
		bg, fg = k.Color("status-yellow-background").Alpha(.42), k.Color("status-yellow-text")
		if k.Dark {
			bg = k.Color("status-yellow-background").Alpha(.26)
		}
	case desktopkit.ToneError:
		bg, fg = k.Color("background-tertiary-error"), k.Color("text-error-primary")
	}
	return ui.Row(c).Padding(9, 16).Gap(8).AlignItems(ui.Center).Background(bg).MinWidth(0).Children(func() {
		k.Icon(c.Key("icon"), icon, 14).TextColor(fg).Shrink(0)
		ui.Text(c.Key("text"), text).FontSize(12).TextColor(fg).Grow(1).MinWidth(0)
	})
}

// settingsFooterBar renders a subtly tinted padded footer action bar inside a SettingsGroup (.s-foot-bar).
func settingsFooterBar(c *ui.Context, k desktopkit.Kit, message string, warn bool, actions func()) ui.Element {
	msgColor := k.Color("text-secondary")
	if warn {
		msgColor = k.Color("status-yellow-text")
	}
	return ui.Row(c).Wrap().Padding(10, 16).GapX(12).GapY(8).AlignItems(ui.Center).Justify(ui.SpaceBetween).
		Background(k.SubtleCardSurface()).MinWidth(0).Children(func() {
		if message != "" {
			ui.Text(c.Key("msg"), message).FontSize(12).TextColor(msgColor).Basis(180).Grow(1).MinWidth(0)
		}
		if actions != nil {
			ui.Row(c.Key("actions")).Wrap().Gap(8).AlignItems(ui.Center).Justify(ui.End).Shrink(0).Children(actions)
		}
	})
}
