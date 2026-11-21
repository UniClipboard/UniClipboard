package boardui

import "github.com/egoist/mygo/ui"

// DressLinkButton applies the pinned link-button.tsx button variant to a native base.
// The caller retains action ownership; this does not create a navigation URL.
func (k Kit) DressLinkButton(c *ui.Context, base ui.Element, label string, p ButtonProps) ui.Element {
	size := normalizedSize(p.Size)
	m := Geometry("button", size)
	style := "body-medium"
	if size == "xs" {
		style = "caption-1-semibold"
	}
	disabled := p.Disabled || p.Busy
	name := label
	if p.Label != "" {
		name = p.Label
	}
	e := base.Label(name).Disabled(disabled).DimDisabled(false).Busy(p.Busy).Gap(4).Radius(4).Padding(0)
	ink := Token(k.Dark, "accent-600")
	if e.Pressed() {
		ink = Token(k.Dark, "accent-800")
	}
	if disabled {
		ink = Token(k.Dark, "text-tertiary")
	}
	dressType(e, style)
	e.TextColor(ink).FocusRingColor(Token(k.Dark, "border-focus-ring")).FocusRingOffset(2)
	e.Children(func() {
		if p.Leading != nil {
			p.Leading(m.IconSize)
		} else if p.Icon != "" {
			ui.Icon(c.Key("leading"), IconSVG(p.Icon)).Size(m.IconSize, m.IconSize).AccessHidden(true)
		}
		if !p.IconOnly {
			t := k.Text(c.Key("label"), label, style).TextColor(ink).SingleLine()
			if e.Hovered() && !disabled {
				t.Underline()
			}
		}
	})
	return e
}
