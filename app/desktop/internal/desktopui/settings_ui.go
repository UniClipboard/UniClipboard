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

// settingsNoteRow renders a quiet, non-alarming contextual note inside a
// SettingsGroup card (used only for static explanatory notes, never errors or warnings).
func settingsNoteRow(c *ui.Context, k desktopkit.Kit, text string) ui.Element {
	fg := k.Color("text-secondary")
	return ui.Row(c).Padding(9, 16).Gap(8).AlignItems(ui.Center).Background(k.SubtleCardSurface()).MinWidth(0).Children(func() {
		k.Icon(c.Key("icon"), desktopkit.IconInfo, 14).TextColor(fg).Shrink(0)
		ui.Text(c.Key("text"), text).FontSize(12).TextColor(fg).Grow(1).MinWidth(0)
	})
}

// subtleInfoCard renders a quiet contextual card for static documentation notes
// (such as plugin privacy details or package-manager update ownership).
func subtleInfoCard(c *ui.Context, k desktopkit.Kit, title, text string) ui.Element {
	fg := k.Color("text-secondary")
	return ui.Row(c).Padding(10, 14).Radius(12).Gap(10).AlignItems(ui.Start).
		Background(k.SubtleCardSurface()).Border(1, k.Color("separator-border")).MinWidth(0).Children(func() {
		k.Icon(c.Key("icon"), desktopkit.IconInfo, 15).TextColor(fg).Margin(2, 0, 0).Shrink(0)
		ui.Column(c.Key("body")).Grow(1).MinWidth(0).Gap(2).Children(func() {
			if title != "" {
				k.Text(c.Key("title"), title, desktopkit.TextProps{Role: desktopkit.RoleCallout, Emphasized: true})
			}
			if text != "" {
				k.Text(c.Key("text"), text, desktopkit.TextProps{Role: desktopkit.RoleCaption, Tone: desktopkit.ToneSecondary})
			}
		})
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
