package desktopkit

import "github.com/egoist/mygo/ui"

type SearchFieldProps struct {
	Label       string  // accessible name
	Value       *string // the query, edited in place
	Placeholder string
	// ShortcutHint is a display-only shortcut such as "Ctrl+F"; registering it is the caller's job.
	ShortcutHint string
	Disabled     bool
}

func (k Kit) SearchField(c *ui.Context, p SearchFieldProps) FieldResult {
	pal := k.fldPal()
	{
	}

	var in ui.Element
	row := ui.Row(c).AlignItems(ui.Center).Gap(8).PaddingX(pal.padX).Role(ui.RoleSearch).MinWidth(160)
	row.Children(func() {
		glyph := func() {
			k.Icon(c.Key("glyph"), IconSearch, 16).TextColor(pal.fg2)
		}
		{
			glyph()
		}
		in = fldBareInput(ui.TextInputBase(c, p.Value).Label(p.Label).Disabled(p.Disabled), pal, p.Placeholder)
		if p.ShortcutHint != "" && (p.Value == nil || *p.Value == "") {
			ui.Text(c, p.ShortcutHint).FontSize(pal.size - 2).TextColor(pal.hint)
		}

	})
	k.fldStyle(row, pal, in.Focused(), false, p.Disabled)

	return FieldResult{Element: row, Input: in, Edited: in.Changed(), Submitted: in.Submitted(),
		Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}

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
func (k Kit) fldRemoveButton(c *ui.Context, label string, ink ui.Color) bool {
	b := ui.ButtonBase(c).Label(label).Size(18, 18).Radius(9).Shrink(0).Center().TextColor(ink).KeepFocus()
	b.Children(func() { k.Icon(c, IconDismiss, 10) })
	if b.Hovered() {
		b.Background(ink.Alpha(.16))
	}
	return b.Clicked()
}
