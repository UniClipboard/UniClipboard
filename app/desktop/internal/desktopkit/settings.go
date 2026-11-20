package desktopkit

import "github.com/egoist/mygo/ui"

type SettingsContentProps struct {
	Label string // accessible name of the page region
}

// SettingsContent is the scrolling page body of a settings category: a centred column of
func (k Kit) SettingsContent(c *ui.Context, p SettingsContentProps, body func()) ui.Element {
	t := k.layTok()
	maxW, gap := float32(800), float32(24)
	pad := []float32{24, 12}
	{
	}

	scroll := k.Scroll(c).Role(ui.RoleRegion).Label(p.Label).Grow(1).MinHeight(0)
	{
		scroll.Background(t.Window)
	}
	return scroll.Children(func() {
		ui.Row(c.Key("page")).Padding(pad...).JustifyItems(ui.Center).Justify(ui.Center).Children(func() {
			ui.Column(c.Key("column")).Grow(1).MaxWidth(maxW).MinWidth(0).Gap(gap).Children(func() {
				if body != nil {
					body()
				}
			})
		})
	})
}

type SettingsGroupProps struct {
	Title string // optional heading above the group
}

func (k Kit) SettingsGroup(c *ui.Context, p SettingsGroupProps, body func()) ui.Element {
	t := k.layTok()
	col := ui.Column(c).Role(ui.RoleGroup).Label(p.Title).MinWidth(0)
	return col.Children(func() {
		if p.Title != "" {
			pad := []float32{0, 2, 8}

			ui.Box(c.Key("title")).Padding(pad...).Children(func() {
				k.Text(c, p.Title, TextProps{Role: RoleCaption, Tone: ToneSecondary, Emphasized: true}).Level(3)
			})
		}
		g := ui.Column(c.Key("body")).MinWidth(0)
		{

			g.Radius(12).Background(t.Card).Border(1, t.Border).Dividers(1, t.Border).Clip()
		}

		g.Children(func() {
			if body != nil {
				body()
			}
		})
	})
}

type SettingsRowProps struct {
	Label       string
	Description string
}

// SettingsRow is one setting: label (plus labelExtra inline, e.g. an "Experimental" badge) and
// optional description on the leading side, control on the trailing side. Either func may be nil.
func (k Kit) SettingsRow(c *ui.Context, p SettingsRowProps, labelExtra func(), control func()) ui.Element {
	hasText := p.Label != "" || p.Description != ""
	row := ui.Row(c).Wrap().AlignItems(ui.Center).Justify(ui.SpaceBetween).GapX(12).GapY(6).MinWidth(0)
	{

		row.MinHeight(56).Padding(8, 12)
		if !hasText {
			row.MinHeight(48)
		}
	}

	return row.Children(func() {
		if hasText {
			ui.Column(c.Key("text")).Basis(224).Grow(1).Shrink(1).MinWidth(0).Gap(2).Children(func() {
				if p.Label != "" {
					ui.Row(c.Key("label")).Wrap().Gap(6).AlignItems(ui.Center).Children(func() {
						k.Text(c.Key("l"), p.Label, TextProps{})
						if labelExtra != nil {
							labelExtra()
						}
					})
				}
				if p.Description != "" {
					k.Text(c.Key("d"), p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary})
				}
			})
		}
		if control != nil {
			r := ui.Row(c.Key("control")).Gap(8).AlignItems(ui.Center).MinWidth(0)
			if hasText {
				r.MarginStart(0)
			} else {
				r.Grow(1)
			}
			r.Children(control)
		}
	})
}
