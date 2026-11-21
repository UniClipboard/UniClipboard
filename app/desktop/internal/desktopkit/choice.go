package desktopkit

import "github.com/egoist/mygo/ui"

type ChoiceListItem struct {
	ID          string
	Title       string
	Description string
	Icon        IconName
}

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

func (k Kit) ChoiceList(c *ui.Context, p ChoiceListProps) ChoiceListResult {
	pal := k.fldPal()
	name := p.Label
	if p.Title != "" {
		name = p.Title
	}
	res := ChoiceListResult{}
	outer := ui.Column(c).Gap(6).Role(ui.RoleGroup).Label(name)

	outer.Children(func() {
		if p.Title != "" {
			t := ui.Text(c.Key("title"), p.Title).FontWeight(700)

			{
				t.FontSize(12).TextColor(pal.fg2).Margin(4, 0, 0, 12)
			}
		}
		var list ui.Element
		{
			bg := k.Color("background-primary-default")

			list = ui.Column(c.Key("group")).Radius(12).Background(bg).Clip()

			{
				k.shadowed(list, "xs")
			}
		}
		body := func() {
			for i, it := range p.Items {
				row := ui.ButtonBase(c.Key(it.ID)).Label(it.Title).Disabled(p.Disabled).AlignItems(ui.Center).Gap(12).
					TextColor(pal.fg).Padding(10, 12)

				switch {
				case p.Disabled:
					row.Opacity(.5)
				case row.Pressed():

					{
						row.Background(pal.press)
					}
				case row.Hovered():
					row.Background(pal.hover)
				}

				row.Children(func() {
					if it.Icon != "" {

						{
							tile := ui.Row(c.Key("tile")).Size(32, 32).Radius(8).Center().Shrink(0).TextColor(k.Color("text-white"))
							switch it.Icon {
							case IconPhone:
								tile.Background(k.Color("notification-success-foreground"))
							case IconPackage:
								tile.Background(k.Color("text-tertiary"))
							default:
								tile.Background(pal.accent)
							}
							tile.Children(func() { k.Icon(c, it.Icon, 18) })
						}
					}
					ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(2).Children(func() {
						ui.Text(c.Key("t"), it.Title).FontSize(pal.size).FontWeight(500).SingleLine()
						if it.Description != "" {
							ui.Text(c.Key("d"), it.Description).FontSize(pal.size - 1.5).TextColor(pal.fg2)
						}
					})
					k.Icon(c.Key("chev"), IconChevronRight, 14).TextColor(pal.hint).Shrink(0)
				})
				if i > 0 {
					row.InsetShadow(0, 1, 0, 0, pal.edge.Alpha(.6))
				}
				if row.Clicked() {
					res.Selected = it.ID
				}
			}
		}

		{
			list.Children(body)
		}
	})
	res.Element = outer
	return res
}
