package uikit

import "github.com/egoist/mygo/ui"

// SearchSuggestion mirrors contract.ts SearchSuggestion: one row of the suggestion menu of a
// TokenSearchField or ComboField.
type SearchSuggestion struct {
	Swatch TagColor // leading colour dot when the suggestion represents a tag
	ID     string
	Label  string
	Group  string // heading shown above this item (the first item of a group)
	Detail string // trailing text, e.g. "12 items · 4 from arch-desktop"
	Kind   SearchTokenKind
	// Selected: already applied. Muted: matches nothing under the other conditions.
	// Create: creates something new ("Create #re").
	Selected, Muted, Create bool
}

// SearchFieldKey is a key the caller's search model decides on (see OnKey of the fields).
type SearchFieldKey struct {
	Key  ui.Key
	Mods ui.Modifiers
}

// Shift reports whether Shift was held.
func (key SearchFieldKey) Shift() bool { return key.Mods&ui.Shift != 0 }

// ComboFieldCreateColor is the colour choice offered on a Create suggestion of a ComboField.
type ComboFieldCreateColor struct {
	Label  string
	Colors []TagColor
	Value  *TagColor // chosen colour, updated when a swatch is pressed
}

// suggestCfg configures fldSuggestions.
type suggestCfg struct {
	label, footer, feedback string
	dots                    bool // swatch dots (ComboField) instead of kind chips (TokenSearchField)
	typed                   string
	hi                      *int
	createColor             *ComboFieldCreateColor
	returnHint              bool // draw the "↩" mark on Create rows
}

// fldPopup styles the surface of a suggestion/menu popup.
func (k Kit) fldPopup(panel ui.Element, pal fldPal, minW float32) ui.Element {
	panel.Background(pal.popup).Radius(pal.popupRadius).Padding(6).MinWidth(minW).TextColor(pal.fg).
		Shadow(0, 6, 18, 0, ui.RGBA(0, 0, 0, .2)).Shadow(0, 0, 0, 1, pal.edge.Alpha(.5))
	return panel
}

// fldSuggestions builds the suggestion list into the popup panel. It returns the index of
// the suggestion clicked in this frame (-1 for none).
func (k Kit) fldSuggestions(c *ui.Context, list []SearchSuggestion, cfg suggestCfg) int {
	pal := k.fldPal()
	picked := -1
	if cfg.feedback != "" {
		ui.Text(c.Key("feedback"), cfg.feedback).FontSize(pal.size-1).TextColor(pal.fg2).Padding(4, 10).Role(ui.RoleStatus)
	}
	list0 := ui.Column(c.Key("list")).Role(ui.RoleListBox).Label(cfg.label)
	list0.Children(func() {
		for i, s := range list {
			c := c.Key(i)
			if s.Group != "" {
				ui.Text(c.Key("group"), s.Group).FontSize(pal.size-2).FontWeight(600).TextColor(pal.fg2).Padding(6, 10, 2)
			}
			label := s.Label
			if s.Create && cfg.typed != "" {
				// the contract bolds the typed name inside "Create #name"; we keep the label whole
				label = s.Label
			}
			row := ui.ButtonBase(c).Label(label).AccessSelected(i == *cfg.hi).Role(ui.RoleOption).KeepFocus().
				Radius(6).Padding(5, 10).Gap(8).AlignItems(ui.Center).TextColor(pal.fg).FontSize(pal.size)
			if row.Hovered() {
				*cfg.hi = i
			}
			if i == *cfg.hi {
				if k.Skin == MacOS {
					row.Background(pal.accent).TextColor(k.Color("on-accent"))
				} else {
					row.Background(pal.hover)
				}
			}
			if s.Muted {
				row.Opacity(.5)
			}
			row.Children(func() {
				switch {
				case s.Create:
					k.Icon(c.Key("add"), IconAdd, 14)
					ui.Text(c, s.Label).SingleLine()
					if cfg.returnHint {
						ui.Text(c.Key("ret"), "↩").FontSize(pal.size - 2).Opacity(.6)
					}
				case cfg.dots:
					ui.Box(c.Key("dot")).Size(10, 10).Radius(5).Shrink(0).Background(k.fldSwatch(s.Swatch))
					ui.Text(c, s.Label).SingleLine()
				default:
					bg, ink := k.fldKindColors(s.Kind)
					h, r, size := k.fldChipMetrics()
					ui.Row(c.Key("chip")).Height(h).PaddingX(9).Radius(r).AlignItems(ui.Center).Background(bg).
						TextColor(ink).FontSize(size).FontWeight(600).Children(func() { ui.Text(c, s.Label).SingleLine() })
				}
				ui.Spacer(c)
				if s.Detail != "" {
					ui.Text(c.Key("detail"), s.Detail).FontSize(pal.size - 2).Opacity(.65).SingleLine()
				}
				if s.Selected {
					ui.Box(c.Key("check")).Size(14, 14).Shrink(0).Draw(func(p *ui.Painter, r ui.Rect) {
						path := (&ui.Path{}).MoveTo(r.X+2, r.Y+7.5).LineTo(r.X+5.5, r.Y+11).LineTo(r.X+12, r.Y+3.5)
						p.StrokePath(path, 1.6, pal.fg)
					})
				}
			})
			if row.Clicked() {
				picked = i
			}
			if s.Create && cfg.createColor != nil && cfg.createColor.Value != nil {
				k.fldCreateColors(c, cfg.createColor, pal)
			}
		}
	})
	if cfg.footer != "" {
		ui.Text(c.Key("footer"), cfg.footer).FontSize(pal.size-2).TextColor(pal.fg2).Padding(6, 10, 2)
	}
	return picked
}

// fldCreateColors draws the colour choice under a Create suggestion as a radio group.
func (k Kit) fldCreateColors(c *ui.Context, cc *ComboFieldCreateColor, pal fldPal) {
	ui.Row(c.Key("colors")).Role(ui.RoleRadioGroup).Label(cc.Label).AlignItems(ui.Center).Gap(6).Padding(2, 10, 6).Children(func() {
		ui.Text(c.Key("colorLabel"), cc.Label).FontSize(pal.size - 2).TextColor(pal.fg2)
		for _, col := range cc.Colors {
			sw := ui.ButtonBase(c.Key(string(col))).Label(string(col)).Role(ui.RoleRadio).AccessSelected(col == *cc.Value).
				KeepFocus().Size(16, 16).Radius(8).Shrink(0).Background(k.fldSwatch(col))
			if col == *cc.Value {
				sw.Border(2, pal.fg)
			}
			if sw.Clicked() {
				*cc.Value = col
			}
		}
	})
}
