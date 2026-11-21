package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type TextFieldProps struct {
	Label                         string
	Value                         *string
	Placeholder                   string
	MaxLength                     int
	Password, AutoFocus, Disabled bool
	Error                         string
}

// TextField preserves the native editor handle and its per-frame edit/submit events.
func (k Kit) TextField(c *ui.Context, p TextFieldProps) FieldResult {
	col := ui.Column(c).MinWidth(0)
	value := p.Value
	if value == nil {
		value = ui.Local(col, "value", func() string { return "" })
	}
	var in ui.Element
	col.Children(func() {
		in = k.native().Input(c.Key("input"), value, boardui.InputProps{Label: p.Label, HideLabel: true, Placeholder: p.Placeholder, MaxLength: p.MaxLength, Password: p.Password, Disabled: p.Disabled, Error: p.Error})
	})
	if p.AutoFocus {
		in.AutoFocus()
	}
	return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted(), Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}
