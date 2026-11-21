package desktopkit

import "github.com/egoist/mygo/ui"

type TextProps struct {
	Role                                   TextRole
	Tone                                   TextTone
	Emphasized, Monospace, Large, Truncate bool
}

func (k Kit) toneColor(t TextTone) ui.Color {
	name := "text-primary"
	switch t {
	case ToneSecondary:
		name = "text-secondary"
	case ToneTertiary:
		name = "text-tertiary"
	case TonePositive:
		name = "status-lime-text"
	case ToneCritical:
		name = "text-error-primary"
	}
	return k.Color(name)
}
func (k Kit) Text(c *ui.Context, s string, p TextProps) ui.Element {
	name := "body-regular"
	switch p.Role {
	case RoleTitle:
		name = "title-2-medium"
	case RoleHeadline:
		name = "headline-medium"
	case RoleCallout:
		name = "body-2-regular"
	case RoleCaption:
		name = "caption-1-regular"
	}
	if p.Emphasized {
		switch p.Role {
		case RoleBody:
			name = "body-medium"
		case RoleHeadline:
			name = "headline-semibold"
		case RoleCallout:
			name = "body-2-medium"
		case RoleCaption:
			name = "caption-1-medium"
		}
	}
	if p.Large {
		name = "display-4-medium"
	}
	e := k.native().Text(c, s, name).TextColor(k.toneColor(p.Tone))
	if p.Monospace {
		e.Font("monospace")
	}
	if p.Truncate {
		e.SingleLine()
	}
	return e
}
