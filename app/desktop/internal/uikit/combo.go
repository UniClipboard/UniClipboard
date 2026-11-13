package uikit

import "github.com/egoist/mygo/ui"

// ComboFieldProps mirrors contract.ts ComboFieldProps: a compact text field with a suggestion
// menu, e.g. the tag field of an item. The caller owns the model (text, suggestions,
// highlighted item, keys); the field reports when the user leaves it (ComboResult.Dismissed).
type ComboFieldProps struct {
	Label       string
	Prefix      string // leading text inside the field, e.g. "#"
	Placeholder string
	Value       *string
	Suggestions []SearchSuggestion
	// Highlighted is the highlighted suggestion (-1 for none); pointer hover writes it, the
	// caller moves it with the arrow keys (via OnKey).
	Highlighted *int
	// OnKey sees each key before the field acts on it; returning true prevents the default.
	OnKey func(SearchFieldKey) bool
	// CreateColor adds a colour choice under a Create suggestion.
	CreateColor *ComboFieldCreateColor
	MaxLength   int
	Footer      string // display-only footer of the menu (keyboard hints)
	// AutoFocus focuses the field when it first appears (the contract's field always does:
	// it opens on demand, e.g. "Add Tag").
	AutoFocus bool
}

// ComboResult is what ComboField reports for one frame.
type ComboResult struct {
	Element, Input ui.Element
	Edited         bool
	Selected       int  // index of the suggestion clicked in this frame, -1 for none
	Dismissed      bool // focus moved away from the field (and its menu)
}

// ComboField is a pill-shaped entry with a menu of suggestions under it.
func (k Kit) ComboField(c *ui.Context, p ComboFieldProps) ComboResult {
	pal := k.fldPal()
	switch k.Skin {
	case MacOS:
		pal.h, pal.size = 22, 12
	case Windows:
		pal.h = 28
	default:
		pal.h = 30
	}
	pal.radius = pal.h / 2
	if k.Skin == Windows {
		pal.radius = 4
	}
	local := -1
	hi := p.Highlighted
	if hi == nil {
		hi = &local
	}
	var in ui.Element
	row := ui.Row(c).AlignItems(ui.Center).Gap(2).PaddingX(pal.padX).MinWidth(140)
	row.Children(func() {
		if p.Prefix != "" {
			ui.Text(c.Key("prefix"), p.Prefix).FontSize(pal.size).TextColor(pal.fg2)
		}
		in = fldBareInput(ui.TextInputBase(c, p.Value).Label(p.Label).Role(ui.RoleComboBox), pal, p.Placeholder)
		if p.MaxLength > 0 {
			in.MaxLength(p.MaxLength)
		}
		if p.AutoFocus {
			in.AutoFocus()
		}
		if p.OnKey != nil {
			on := p.OnKey
			in.OnTextKey(func(tk ui.TextKey) bool { return on(SearchFieldKey{Key: tk.Key, Mods: tk.Mods}) })
		}
	})
	focused := in.Focused()
	k.fldStyle(row, pal, focused, false, false)
	if k.Skin == MacOS && focused {
		row.InsetShadow(0, 0, 0, 1, pal.accent)
	}
	if k.Skin == Linux && !focused {
		row.InsetShadow(0, 0, 0, 2, pal.focus) // Adwaita keeps the tag field ringed
	}

	was := ui.Local(in, "uikit.focus", func() bool { return false })
	dismissed := *was && !focused
	*was = focused

	open := focused && len(p.Suggestions) > 0
	res := ComboResult{Element: row, Input: in, Selected: -1, Dismissed: dismissed, Edited: in.Changed()}
	typed := ""
	if p.Value != nil {
		typed = *p.Value
	}
	ui.PopoverBase(c, row, &open, func(panel ui.Element) {
		width := max(row.Bounds().W, 220)
		if k.Skin == MacOS {
			width = 300
			pal.popupRadius = 14
		}
		k.fldPopup(panel, pal, width)
		res.Selected = k.fldSuggestions(c, p.Suggestions, suggestCfg{
			label: p.Label, footer: p.Footer, dots: true, typed: typed, hi: hi,
			createColor: p.CreateColor, returnHint: true,
		})
	})
	return res
}
