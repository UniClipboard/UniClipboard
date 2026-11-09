package uikit

import "github.com/egoist/mygo/ui"

// FormFieldProps mirrors contract.ts FormFieldProps: a labelled single-line field inside a form.
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
	pal := k.fldPal()
	var in ui.Element
	col := ui.Column(c).Gap(6).MinWidth(200)
	var ctl ui.Element
	col.Children(func() {
		lw := 400
		if k.Skin == MacOS {
			lw = 500
		}
		ui.Text(c.Key("label"), p.Label).FontSize(pal.size).FontWeight(lw + 200).TextColor(pal.fg)
		ctl = ui.Row(c.Key("control")).AlignItems(ui.Center).PaddingX(pal.padX)
		ctl.Children(func() {
			shown := ui.Local(ctl, "uikit.revealed", func() bool { return false })
			in = ui.TextInputBase(c, p.Value).Label(p.Label).Disabled(p.Disabled)
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
			k.Text(c.Key("error"), p.Error, TextProps{Role: RoleCaption, Tone: ToneCritical}).Role(ui.RoleAlert)
		}
		if p.Hint != "" {
			k.Text(c.Key("hint"), p.Hint, TextProps{Role: RoleCaption, Tone: ToneSecondary})
		}
	})
	return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted(),
		Escaped: in.Focused() && in.Shortcut(0, ui.KeyEscape)}
}
