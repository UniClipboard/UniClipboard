package uikit

import "github.com/egoist/mygo/ui"

// TextFieldProps mirrors contract.ts TextFieldProps: a single-line text field, e.g.
// renaming in place. Return reports FieldResult.Submitted, Escape FieldResult.Escaped.
type TextFieldProps struct {
	Label       string  // accessible name
	Value       *string // edited in place
	Placeholder string
	MaxLength   int  // 0 = unlimited (counted in UTF-16 units like HTML maxlength)
	AutoFocus   bool // focus the field when it appears
	Disabled    bool
	// Error is shown under the field (e.g. "#design already exists.") and marks it invalid.
	Error string
}

// TextField is a single-line entry with the skin's focus indication. Text editing, selection
// and input-method composition come from mygo's TextInputBase; Return does not submit while
// an input method is composing.
func (k Kit) TextField(c *ui.Context, p TextFieldProps) FieldResult {
	pal := k.fldPal()
	if k.Skin == MacOS {
		pal.h, pal.radius, pal.padX = 22, 5, 6
	}
	var in ui.Element
	col := ui.Column(c).Gap(2).MinWidth(0)
	col.Children(func() {
		in = ui.TextInputBase(c, p.Value).Label(p.Label).Disabled(p.Disabled)
		in.FontSize(pal.size).Height(pal.h).PaddingX(pal.padX).Placeholder(p.Placeholder).PlaceholderColor(pal.hint)
		if p.MaxLength > 0 {
			in.MaxLength(p.MaxLength)
		}
		if p.AutoFocus {
			in.AutoFocus()
		}
		k.fldStyle(in, pal, in.Focused(), p.Error != "", p.Disabled)
		if p.Error != "" {
			k.Text(c, p.Error, TextProps{Role: RoleCaption, Tone: ToneCritical}).Role(ui.RoleAlert)
		}
	})
	return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted(),
		Escaped: p.Value != nil && in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}
