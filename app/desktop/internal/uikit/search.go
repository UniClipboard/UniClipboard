package uikit

import "github.com/egoist/mygo/ui"

// SearchFieldProps mirrors contract.ts SearchFieldProps.
type SearchFieldProps struct {
	Label       string  // accessible name
	Value       *string // the query, edited in place
	Placeholder string
	// ShortcutHint is a display-only shortcut such as "Ctrl+F"; registering it is the caller's job.
	ShortcutHint string
	Disabled     bool
}

// SearchField is a query entry: query glyph (leading on Adwaita and macOS, trailing on
// Windows), the text, and a shortcut hint while empty. Typing settles in FieldResult.Edited;
// Return is FieldResult.Submitted (search now instead of waiting for typing to settle).
func (k Kit) SearchField(c *ui.Context, p SearchFieldProps) FieldResult {
	pal := k.fldPal()
	switch k.Skin {
	case MacOS:
		pal.h, pal.radius, pal.size, pal.padX, pal.bg = 32, 16, 13, 12, pal.fill
	case Windows:
		pal.padX = 6
	}
	var in ui.Element
	row := ui.Row(c).AlignItems(ui.Center).Gap(8).PaddingX(pal.padX).Role(ui.RoleSearch).MinWidth(160)
	row.Children(func() {
		glyph := func() {
			k.Icon(c.Key("glyph"), IconSearch, 16).TextColor(pal.fg2)
		}
		if k.Skin != Windows {
			glyph()
		}
		in = fldBareInput(ui.TextInputBase(c, p.Value).Label(p.Label).Disabled(p.Disabled), pal, p.Placeholder)
		if p.ShortcutHint != "" && (p.Value == nil || *p.Value == "") {
			ui.Text(c, p.ShortcutHint).FontSize(pal.size - 2).TextColor(pal.hint)
		}
		if k.Skin == Windows {
			glyph()
		}
	})
	k.fldStyle(row, pal, in.Focused(), false, p.Disabled)
	if k.Skin == MacOS {
		row.Background(ui.Transparent).Border(0, ui.Transparent).NoShadow()
		if in.Focused() {
			row.Shadow(0, 0, 0, 3.5, pal.focus)
		}
		k.glassSurface(row)
	}
	return FieldResult{Element: row, Input: in, Edited: in.Changed(), Submitted: in.Submitted(),
		Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}

// SearchTokenProps mirrors contract.ts SearchTokenProps: a condition chip such as
// "from: iPhone 16".
type SearchTokenProps struct {
	Field, Value string
	Kind         SearchTokenKind
	// RemoveLabel is the accessible name of the remove button ("Remove from: iPhone 16");
	// empty draws a chip without one.
	RemoveLabel string
}

// SearchTokenResult is what SearchToken reports.
type SearchTokenResult struct {
	Element ui.Element
	Removed bool // the remove button was pressed in this frame
}

// SearchToken is a read-only condition chip, optionally removable.
func (k Kit) SearchToken(c *ui.Context, p SearchTokenProps) SearchTokenResult {
	h, radius, size := k.fldChipMetrics()
	bg, ink := k.fldKindColors(p.Kind)
	removed := false
	chip := ui.Row(c).Height(h).AlignItems(ui.Center).Gap(4).PaddingX(9).Radius(radius).
		Background(bg).TextColor(ink).FontSize(size).Shrink(1).MinWidth(0)
	chip.Children(func() {
		ui.Text(c, p.Field).Opacity(.75).SingleLine()
		ui.Text(c, p.Value).FontWeight(600).SingleLine().MinWidth(0).Tooltip(p.Value)
		if p.RemoveLabel != "" {
			removed = k.fldRemoveButton(c, p.RemoveLabel, ink)
		}
	})
	if p.RemoveLabel != "" {
		chip.PaddingEnd(3)
	}
	return SearchTokenResult{Element: chip, Removed: removed}
}

// fldRemoveButton is the small round "x" inside chips and tokens. It never takes keyboard focus
// or steals it from a field (pointer-only, like AppKit's token remove).
func (k Kit) fldRemoveButton(c *ui.Context, label string, ink ui.Color) bool {
	b := ui.ButtonBase(c).Label(label).Size(18, 18).Radius(9).Shrink(0).Center().TextColor(ink).KeepFocus()
	b.Children(func() { k.Icon(c, IconDismiss, 10) })
	if b.Hovered() {
		b.Background(ink.Alpha(.16))
	}
	return b.Clicked()
}
