package uikit

import "github.com/egoist/mygo/ui"

// ContentUnavailableProps mirrors contract.ts ContentUnavailableProps.
type ContentUnavailableProps struct {
	Icon        IconName // optional
	Title       string
	Description string // optional
}

// ContentUnavailable is the empty, no-results or placeholder state of a pane (AppKit "content
// unavailable", AdwStatusPage). actions builds optional actions or choices below the description.
func (k Kit) ContentUnavailable(c *ui.Context, p ContentUnavailableProps, actions func()) ui.Element {
	t := k.layTok()
	iconSize := float32(48)
	e := ui.Column(c).Role(ui.RoleStatus).Label(p.Title).Grow(1).Padding(24).Gap(6).
		AlignItems(ui.Center).Justify(ui.Center)
	return e.Children(func() {
		if p.Icon != "" {
			ui.Box(c.Key("icon")).Margin(0, 0, 8).TextColor(t.Fg3).AccessHidden(true).Children(func() {
				k.Icon(c, p.Icon, iconSize)
			})
		}
		title := k.Text(c.Key("title"), p.Title, TextProps{Role: RoleTitle, Emphasized: true}).TextAlign(ui.Center)
		if k.Skin == MacOS {
			title.FontSize(17).FixedLineHeight(22)
		}
		if p.Description != "" {
			ui.Box(c.Key("desc")).MaxWidth(360).Children(func() {
				k.Text(c, p.Description, TextProps{Role: RoleCallout, Tone: ToneSecondary}).TextAlign(ui.Center)
			})
		}
		if actions != nil {
			ui.Column(c.Key("actions")).Margin(14, 0, 0).Gap(8).MinWidth(280).AlignItems(ui.Stretch).Children(actions)
		}
	})
}
