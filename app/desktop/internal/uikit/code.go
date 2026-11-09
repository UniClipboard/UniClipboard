package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// CodeFieldProps mirrors contract.ts CodeFieldProps: a short numeric code typed into
// separate boxes, split into two halves (482-913). Only digits are kept, so a pasted
// "482-913" fills every box.
type CodeFieldProps struct {
	Label     string
	Length    int     // number of boxes (>0)
	Value     *string // digits only, at most Length
	Invalid   bool
	Disabled  bool
	AutoFocus bool
}

// fldDigits keeps the first n digits, normalizing full-width input.
func fldDigits(s string, n int) string {
	var b strings.Builder
	for _, r := range s {
		if r >= '０' && r <= '９' {
			r = r - '０' + '0'
		}
		if r >= '0' && r <= '9' && b.Len() < n {
			b.WriteRune(r)
		}
	}
	return b.String()
}

// CodeField draws the boxes; one text input underneath (invisible, covering them) takes the
// typing, paste and input methods. It returns the frame's Edited state.
func (k Kit) CodeField(c *ui.Context, p CodeFieldProps) FieldResult {
	pal := k.fldPal()
	n := max(p.Length, 1)
	if p.Value == nil {
		p.Value = new(string)
	}
	*p.Value = fldDigits(*p.Value, n)
	var in ui.Element
	col := ui.Column(c).Gap(8)
	col.Children(func() {
		if k.Skin == Windows {
			ui.Text(c.Key("label"), p.Label).FontSize(pal.size).TextColor(pal.fg)
		}
		stack := ui.Box(c.Key("stack"))
		stack.Children(func() {
			w, h, r, fs := float32(40), float32(48), float32(8), float32(22)
			if k.Skin == MacOS {
				w, h, r = 38, 46, 9
			}
			slotPal := pal
			slotPal.radius = r
			cur := min(len(*p.Value), n-1)
			// the input comes first in build order, so the slots can read its focus below
			in = ui.TextInputBase(c.Key("input"), p.Value).Label(p.Label).Disabled(p.Disabled)
			in.Absolute().Left(0).Top(0).Right(0).Bottom(0).ZIndex(1).Background(ui.Transparent).
				TextColor(ui.Transparent).CaretWidth(0).Cursor(ui.CursorText)
			in.OnPaste(func(text string) bool {
				*p.Value = fldDigits(text, n)
				return true
			})
			if p.AutoFocus {
				in.AutoFocus()
			}
			focused := in.Focused()
			ui.Row(c.Key("boxes")).AlignItems(ui.Center).Gap(6).Children(func() {
				for i := 0; i < n; i++ {
					if i > 0 && n%2 == 0 && i == n/2 {
						ui.Box(c.Key("sep")).Size(10, 2).Radius(1).Background(pal.hint).MarginX(4)
					}
					slot := ui.Row(c.Key(i)).Size(w, h).Center().FontSize(fs).FontWeight(700).TextColor(pal.fg)
					digit := ""
					if i < len(*p.Value) {
						digit = (*p.Value)[i : i+1]
					}
					slot.Children(func() {
						if digit != "" {
							ui.Text(c, digit)
						} else if focused && i == cur && !p.Disabled {
							ui.Box(c.Key("caret")).Size(1.5, 22).Background(pal.accent)
						}
					})
					k.fldStyle(slot, slotPal, focused && i == cur, p.Invalid, p.Disabled)
					if p.Invalid {
						slot.TextColor(pal.danger)
					}
				}
			})
		})
	})
	if d := fldDigits(*p.Value, n); d != *p.Value {
		*p.Value = d
	}
	return FieldResult{Element: col, Input: in, Edited: in.Changed(), Submitted: in.Submitted()}
}
