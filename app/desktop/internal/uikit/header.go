package uikit

import "github.com/egoist/mygo/ui"

// DetailHeaderProps mirrors contract.ts DetailHeaderProps. Linux and macOS show the pin and
// delete commands elsewhere (macOS toolbar, the pane action bar), so only Windows draws them,
// as subtle icon buttons after the metadata.
type DetailHeaderProps struct {
	PinLabel, PinTooltip string
	DeleteLabel          string
}

// DetailHeaderResult carries the command clicks of the frame.
type DetailHeaderResult struct {
	ui.Element
	PinClicked, DeleteClicked bool
}

// DetailHeader is the metadata row of a detail pane (body) with the commands presented
// according to the platform's layout.
func (k Kit) DetailHeader(c *ui.Context, p DetailHeaderProps, body func()) DetailHeaderResult {
	var r DetailHeaderResult
	row := ui.Row(c).Gap(8).AlignItems(ui.Center).Shrink(0)
	r.Element = row
	row.Children(func() {
		if body != nil {
			body()
		}
		if k.Skin != Windows {
			return
		}
		if p.PinLabel != "" {
			b := k.Button(c.Key("pin"), "", ButtonProps{Variant: Subtle, Icon: IconPin, Label: p.PinLabel, Tooltip: p.PinTooltip})
			r.PinClicked = b.Clicked()
		}
		if p.DeleteLabel != "" {
			b := k.Button(c.Key("delete"), "", ButtonProps{Variant: Subtle, Icon: IconDelete, Label: p.DeleteLabel})
			r.DeleteClicked = b.Clicked()
		}
	})
	return r
}

// EntityStatus is the status line of an EntityHeader.
type EntityStatus struct {
	Text   string
	Tone   StatusTone
	Detail string
}

// EntityHeaderProps mirrors contract.ts EntityHeaderProps.
type EntityHeaderProps struct {
	Icon IconName
	// Pending draws an outlined icon for something that is not an object yet, e.g. an invitation.
	Pending bool
	Title   string
	// Badge is a small label after the title, e.g. "This device".
	Badge string
	// Status is shown when Status.Text is not empty.
	Status EntityStatus
	// Description is a longer explanation under the status line.
	Description string
	// Centered stacks icon, title, status and actions in the middle (a hero, as in System
	// Settings), instead of the leading row.
	Centered bool
}

// EntityHeader is the heading of an object's detail, e.g. a device: a large icon, the name, a
// status line and the object's commands (actions) on the trailing side.
func (k Kit) EntityHeader(c *ui.Context, p EntityHeaderProps, actions func()) ui.Element {
	if p.Centered {
		return k.entityHeaderCentered(c, p, actions)
	}
	t := k.layTok()
	tile := float32(56)
	if k.Skin == MacOS {
		tile = 48
	}
	row := ui.Row(c).Gap(16).AlignItems(ui.Center)
	return row.Children(func() {
		box := ui.Box(c.Key("tile")).Size(tile, tile).Shrink(0).Radius(12).Center().
			Justify(ui.Center).AlignItems(ui.Center).AccessHidden(true)
		if p.Pending {
			box.InsetShadow(0, 0, 0, 1.5, t.Border).TextColor(t.Accent)
		} else {
			box.Background(t.Shade).TextColor(t.Fg)
		}
		box.Children(func() { k.Icon(c, p.Icon, 30) })

		ui.Column(c.Key("text")).Grow(1).MinWidth(0).Gap(4).Children(func() {
			ui.Row(c.Key("title-row")).Gap(8).AlignItems(ui.Center).MinWidth(0).Children(func() {
				k.Text(c.Key("title"), p.Title, TextProps{Role: RoleTitle, Emphasized: true, Truncate: true}).Level(2).Tooltip(p.Title)
				if p.Badge != "" {
					ui.Row(c.Key("badge")).Height(22).Padding(0, 9).Radius(11).Background(t.Shade).
						AlignItems(ui.Center).Shrink(0).Children(func() {
						k.Text(c, p.Badge, TextProps{Role: RoleCaption, Tone: ToneSecondary, Emphasized: true})
					})
				}
			})
			if p.Status.Text != "" {
				ui.Row(c.Key("status")).Gap(6).AlignItems(ui.Center).Children(func() {
					ui.Box(c.Key("dot")).Size(8, 8).Radius(4).Background(k.layStatusColor(p.Status.Tone)).AccessHidden(true)
					k.Text(c.Key("st"), p.Status.Text, TextProps{Role: RoleCallout, Emphasized: true})
					if p.Status.Detail != "" {
						k.Text(c.Key("sep"), "·", TextProps{Role: RoleCallout, Tone: ToneSecondary})
						k.Text(c.Key("sd"), p.Status.Detail, TextProps{Role: RoleCallout, Tone: ToneSecondary})
					}
				})
			}
			if p.Description != "" {
				k.Text(c.Key("desc"), p.Description, TextProps{Role: RoleCallout, Tone: ToneSecondary})
			}
		})
		if actions != nil {
			ui.Row(c.Key("actions")).Gap(8).Shrink(0).AlignItems(ui.Center).Children(actions)
		}
	})
}

func (k Kit) entityHeaderCentered(c *ui.Context, p EntityHeaderProps, actions func()) ui.Element {
	t := k.layTok()
	return ui.Column(c).Gap(6).AlignItems(ui.Center).Padding(4, 0, 8).Children(func() {
		box := ui.Box(c.Key("tile")).Size(64, 64).Shrink(0).Radius(16).Center().
			Justify(ui.Center).AlignItems(ui.Center).AccessHidden(true)
		if p.Pending {
			box.InsetShadow(0, 0, 0, 1.5, t.Border).TextColor(t.Accent)
		} else {
			box.Background(t.Shade).TextColor(t.Fg)
		}
		box.Children(func() { k.Icon(c, p.Icon, 34) })
		ui.Box(c.Key("gap")).Size(1, 4)
		k.Text(c.Key("title"), p.Title, TextProps{Role: RoleTitle, Emphasized: true, Truncate: true}).Level(2)
		if p.Status.Text != "" {
			ui.Row(c.Key("status")).Gap(6).AlignItems(ui.Center).Children(func() {
				ui.Box(c.Key("dot")).Size(7, 7).Radius(4).Background(k.layStatusColor(p.Status.Tone)).AccessHidden(true)
				k.Text(c.Key("st"), p.Status.Text, TextProps{Role: RoleCallout, Tone: ToneSecondary})
			})
		}
		if p.Description != "" {
			k.Text(c.Key("desc"), p.Description, TextProps{Role: RoleCaption, Tone: ToneSecondary})
		}
		if actions != nil {
			ui.Row(c.Key("actions")).Gap(8).Padding(8, 0, 0).AlignItems(ui.Center).Children(actions)
		}
	})
}
