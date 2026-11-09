package uikit

import "github.com/egoist/mygo/ui"

// ButtonRow lays out buttons inside page content with a gap, wrapping when narrow: centred on
// Linux and macOS (entry pages), at the leading edge on Windows.
func (k Kit) ButtonRow(c *ui.Context, body func()) ui.Element {
	e := ui.Row(c).Wrap().Gap(8).AlignItems(ui.Center)
	if k.Skin != Windows {
		e.Justify(ui.Center)
	}
	return e.Children(body)
}

// RemoveButton is the small "x" control of a tag or search token: a button of its own, named
// by the caller's label. Read the click with .Clicked().
func (k Kit) RemoveButton(c *ui.Context, label string) ui.Element {
	e := ui.ButtonBase(c).Label(label).Tooltip(label).Radius(4)
	if k.Skin == Windows {
		e.PaddingX(2)
	}
	op := float32(.7)
	if e.Hovered() {
		op = 1
	}
	return e.Opacity(op).Children(func() {
		k.glyph(c, "close", 10)
	})
}
