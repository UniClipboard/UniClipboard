package uikit

import "github.com/egoist/mygo/ui"

// SelectItem mirrors contract.ts SelectItem.
type SelectItem struct{ Value, Label string }

// SelectProps mirrors contract.ts SelectProps: a single choice from a short list (Windows
// drop-down, macOS pop-up button, Adwaita drop-down).
type SelectProps struct {
	Items []SelectItem
	// Value is the chosen item's Value; a value that is not one of the items means nothing is
	// chosen and Placeholder shows. It is updated when the user picks an item.
	Value       *string
	Label       string // accessible name
	Placeholder string
	Disabled    bool
	// Filter narrows the list in view (a filter pill in the list's filter row); Placeholder is
	// the filter's name while nothing is chosen, and a chosen value turns the pill accent-tinted.
	Filter bool
}

// SelectResult is what Select reports.
type SelectResult struct {
	Element ui.Element // the trigger
	Changed bool       // the user chose another item in this frame
}

// fldChevron draws a small down chevron in col.
func fldChevron(c *ui.Context, size float32, col ui.Color) ui.Element {
	return ui.Box(c).Size(size, size).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
		path := (&ui.Path{}).MoveTo(r.X+r.W*.2, r.Y+r.H*.38).LineTo(r.X+r.W*.5, r.Y+r.H*.66).LineTo(r.X+r.W*.8, r.Y+r.H*.38)
		p.StrokePath(path, 1.4, col)
	})
}

// Select is a drop-down choosing one of Items. Keyboard: arrows open the popup and move among
// the options, Home/End jump, Return/Space choose, Escape closes (mygo's SelectBase).
func (k Kit) Select(c *ui.Context, p SelectProps) SelectResult {
	pal := k.fldPal()
	local := ""
	if p.Value == nil {
		p.Value = &local
	}
	chosen := -1
	for i, it := range p.Items {
		if it.Value == *p.Value {
			chosen = i
		}
	}
	sel := ui.SelectBase(c, p.Value)
	tr := sel.Trigger.Label(p.Label).Disabled(p.Disabled).FontSize(pal.size).AlignItems(ui.Center).Gap(8).TextColor(pal.fg)
	open := sel.Open()
	h, padL, padR := pal.h, pal.padX+2, pal.padX
	bg := pal.bg
	switch k.Skin {
	case Linux:
		bg = pal.bg
		tr.FontWeight(700)
		if tr.Hovered() {
			bg = k.Color("btn-hover")
		}
		if open {
			bg = pal.press
		}
		tr.Background(bg).Radius(pal.radius)
	case MacOS:
		h, padL, padR = 24, 10, 6
		tr.Background(k.Color("ctrl")).Radius(7).Shadow(0, .5, 1, 0, ui.RGBA(0, 0, 0, .25)).Shadow(0, 0, 0, .5, pal.edge)
		if tr.Focused() {
			tr.Shadow(0, 0, 0, 3.5, pal.focus)
		}
	default:
		tr.Background(bg).Border(1, pal.edge)
		if tr.Hovered() {
			tr.Background(k.Color("colorNeutralBackground1Hover"))
		}
		tr.Radius(pal.radius)
		switch {
		case tr.Focused() || open:
			tr.InsetShadow(0, -2, 0, 0, pal.accent)
		default:
			tr.InsetShadow(0, -1, 0, 0, pal.underline)
		}
	}
	text, textCol := p.Placeholder, pal.hint
	if chosen >= 0 {
		text, textCol = p.Items[chosen].Label, pal.fg
	}
	if p.Filter {
		h, padL, padR = 26, 10, 6
		if chosen < 0 {
			textCol = pal.fg
		}
		fillBg := pal.fill
		if k.Skin == Windows {
			fillBg = k.Color("colorNeutralBackground3")
		}
		tr.Background(fillBg).Radius(h / 2)
		if k.Skin == Windows {
			tr.Radius(4)
		}
		if chosen >= 0 {
			accentBg, accentInk := k.fldKindColors(SearchKindDefault)
			tr.Background(accentBg)
			textCol = accentInk
			if tr.Hovered() {
				tr.Background(accentInk.Alpha(.2))
			}
		}
	}
	tr.Height(h).PaddingStart(padL).PaddingEnd(padR).Opacity(map[bool]float32{true: .5, false: 1}[p.Disabled])
	tr.Children(func() {
		ui.Text(c.Key("value"), text).TextColor(textCol).SingleLine().Grow(1)
		fldChevron(c.Key("chevron"), 12, pal.fg2)
	})
	w := tr.Bounds().W
	sel.Popup(func(panel ui.Element) {
		k.fldPopup(panel, pal, max(w, 120)).Padding(4)
		for i, it := range p.Items {
			row := sel.Item(it.Value).Label(it.Label).Radius(5).Padding(6, 10).AlignItems(ui.Center).Gap(8).TextColor(pal.fg).FontSize(pal.size)
			ink := pal.fg
			if row.Highlighted() {
				if k.Skin == MacOS {
					ink = k.Color("on-accent")
					row.Background(pal.accent).TextColor(ink)
				} else {
					row.Background(pal.hover)
				}
			}
			row.Children(func() {
				ui.Text(c.Key(i), it.Label).SingleLine().Grow(1)
				if i == chosen {
					ui.Box(c.Key("check")).Size(14, 14).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
						path := (&ui.Path{}).MoveTo(r.X+2, r.Y+7.5).LineTo(r.X+5.5, r.Y+11).LineTo(r.X+12, r.Y+3.5)
						p.StrokePath(path, 1.6, ink)
					})
				}
			})
		}
	})
	return SelectResult{Element: tr, Changed: tr.Changed()}
}
