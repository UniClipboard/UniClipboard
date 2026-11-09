package uikit

import "github.com/egoist/mygo/ui"

// ChoiceListItem mirrors contract.ts ChoiceListItem.
type ChoiceListItem struct {
	ID          string
	Title       string
	Description string
	Icon        IconName
}

// ChoiceListProps mirrors contract.ts ChoiceListProps: choices that each lead to the next step
// (macOS grouped rows with a chevron, WinUI SettingsCards).
type ChoiceListProps struct {
	Label    string // accessible name when Title is empty
	Title    string // heading above the group, e.g. "Move or recover"
	Items    []ChoiceListItem
	Disabled bool
}

// ChoiceListResult is what ChoiceList reports.
type ChoiceListResult struct {
	Element  ui.Element
	Selected string // ID of the item activated in this frame, "" for none
}

// ChoiceList draws one button per choice. Adwaita and macOS group the rows in one rounded
// card with hairlines between them; Windows draws separate cards.
func (k Kit) ChoiceList(c *ui.Context, p ChoiceListProps) ChoiceListResult {
	pal := k.fldPal()
	name := p.Label
	if p.Title != "" {
		name = p.Title
	}
	res := ChoiceListResult{}
	outer := ui.Column(c).Gap(6).Role(ui.RoleGroup).Label(name)
	win := k.Skin == Windows
	if win {
		outer.Gap(4)
	}
	outer.Children(func() {
		if p.Title != "" {
			t := ui.Text(c.Key("title"), p.Title).FontWeight(700)
			if win {
				t.FontSize(14).FontWeight(600).TextColor(pal.fg).Margin(12, 0, 4)
			} else {
				t.FontSize(12).TextColor(pal.fg2).Margin(4, 0, 0, 12)
			}
		}
		list := outer
		if !win {
			bg := k.Color("card")
			if k.Skin == MacOS {
				bg = k.Color("content")
			}
			list = ui.Column(c.Key("group")).Radius(12).Background(bg).Clip()
			if k.Skin == MacOS {
				list.Shadow(0, 0, 0, .5, pal.edge)
			} else {
				list.Shadow(0, 1, 3, 0, ui.RGBA(0, 0, 0, .12))
			}
		}
		body := func() {
			for i, it := range p.Items {
				row := ui.ButtonBase(c.Key(it.ID)).Label(it.Title).Disabled(p.Disabled).AlignItems(ui.Center).Gap(12).
					TextColor(pal.fg).Padding(10, 12)
				if win {
					row.Radius(4).Padding(12, 16).Gap(16).MinHeight(68).Background(k.Color("w11-card")).Border(1, k.Color("w11-card-stroke"))
				}
				switch {
				case p.Disabled:
					row.Opacity(.5)
				case row.Pressed():
					if win {
						row.Background(pal.press)
					} else {
						row.Background(pal.press)
					}
				case row.Hovered():
					row.Background(pal.hover)
				}
				if row.Focused() && win {
					row.Shadow(0, 0, 0, 2, pal.focus)
				}
				row.Children(func() {
					if it.Icon != "" {
						if win {
							k.Icon(c.Key("icon"), it.Icon, 20).Shrink(0)
						} else {
							tile := ui.Row(c.Key("tile")).Size(32, 32).Radius(8).Center().Shrink(0).TextColor(ui.RGB(255, 255, 255))
							switch it.Icon {
							case IconPhone:
								tile.Background(k.Color("green"))
							case IconPackage:
								tile.Background(k.Color("gray"))
							default:
								tile.Background(pal.accent)
							}
							tile.Children(func() { k.Icon(c, it.Icon, 18) })
						}
					}
					ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Text(c.Key("t"), it.Title).FontSize(pal.size).FontWeight(map[bool]int{true: 400, false: 700}[win]).SingleLine()
						if it.Description != "" {
							ui.Text(c.Key("d"), it.Description).FontSize(pal.size - 1.5).TextColor(pal.fg2)
						}
					})
					k.Icon(c.Key("chev"), IconChevronRight, 14).TextColor(pal.hint).Shrink(0)
				})
				if !win && i > 0 {
					row.InsetShadow(0, 1, 0, 0, pal.edge.Alpha(.6))
				}
				if row.Clicked() {
					res.Selected = it.ID
				}
			}
		}
		if win {
			body()
		} else {
			list.Children(body)
		}
	})
	res.Element = outer
	return res
}
