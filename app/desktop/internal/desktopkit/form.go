package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type FormFieldProps struct {
	Label string // visible label above the field
	Value *string
	// Password masks the value and adds a show/hide button named by RevealShow / RevealHide.
	Password            bool
	RevealShow          string
	RevealHide          string
	Placeholder         string
	Hint                string // help under the field
	Error               string // shown under the field, marks it invalid
	Disabled, AutoFocus bool
}

// FormField is a labelled field: label above, control (with reveal button for passwords), then
// the error and the hint. Return is FieldResult.Submitted (not while composing).
func (k Kit) FormField(c *ui.Context, p FormFieldProps) FieldResult {
	if !p.Password {
		var in ui.Element
		col := ui.Column(c).MinWidth(0)
		col.Children(func() {
			in = k.native().Input(c.Key("input"), p.Value, boardui.InputProps{Label: p.Label, Placeholder: p.Placeholder, Description: p.Hint, Error: p.Error, Disabled: p.Disabled})
		})
		if p.AutoFocus {
			in.AutoFocus()
		}
		return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted(), Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
	}
	pal := k.fldPal()
	var in ui.Element
	col := ui.Column(c).Gap(boardui.Number("input-field-gap")).MinWidth(0)
	var ctl ui.Element
	col.Children(func() {
		label := k.native().Text(c.Key("label"), p.Label, "body-medium")
		ctl = ui.Row(c.Key("control")).AlignItems(ui.Center).PaddingX(pal.padX)
		ctl.Children(func() {
			shown := ui.Local(ctl, "desktopkit.revealed", func() bool { return false })
			in = ui.TextInputBase(c, p.Value).Label(p.Label).LabelledBy(label).Disabled(p.Disabled).DimDisabled(false).Invalid(p.Error != "")
			fldBareInput(in, pal, p.Placeholder)
			if p.Password && !*shown {
				in.Password()
			}
			if p.AutoFocus {
				in.AutoFocus()
			}
			if p.Password {
				name, icon := p.RevealShow, IconLock
				if *shown {
					name, icon = p.RevealHide, IconUnlock
				}
				b := ui.ButtonBase(c.Key("reveal")).Label(name).Disabled(p.Disabled).Size(26, 26).Radius(6).Shrink(0).Center().
					TextColor(pal.fg2).Tooltip(name)
				b.Children(func() { k.Icon(c, icon, 16) })
				if b.Hovered() {
					b.Background(pal.hover)
				}
				if b.Clicked() {
					*shown = !*shown
				}
			}
		})
		k.fldStyle(ctl, pal, in.Focused(), p.Error != "", p.Disabled)
		if p.Error != "" {
			err := k.Text(c.Key("error"), p.Error, TextProps{Role: RoleCaption, Tone: ToneCritical}).Role(ui.RoleAlert)
			in.ErrorMessage(err)
		}
		if p.Hint != "" && p.Error == "" {
			hint := k.Text(c.Key("hint"), p.Hint, TextProps{Role: RoleCaption, Tone: ToneSecondary})
			in.DescribedBy(hint)
		}
	})
	return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted(),
		Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}
