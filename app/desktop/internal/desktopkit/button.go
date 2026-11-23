package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

type ButtonProps struct {
	Variant        ButtonVariant
	Size           ControlSize
	Icon           IconName
	Label, Tooltip string
	Disabled, Busy bool
}

func (k Kit) Button(c *ui.Context, label string, p ButtonProps) ui.Element {
	return k.dressButton(c, ui.ButtonBase(c), label, p, false)
}

// dressButton retains toggle/menu behavior while reusing the public Button paint.
// Subtle buttons use the shared quiet neutral presentation (transparent at rest) rather
// than BoardUI's accent-tinted ghost variant.
func (k Kit) dressButton(c *ui.Context, e ui.Element, label string, p ButtonProps, on bool) ui.Element {
	if p.Variant == Subtle && !on {
		return k.dressQuietButton(c, e, label, quietButtonProps{
			Size:     p.Size,
			Icon:     p.Icon,
			Label:    p.Label,
			Tooltip:  p.Tooltip,
			Disabled: p.Disabled,
			Busy:     p.Busy,
		})
	}
	size := "medium"
	if p.Size == SizeSmall {
		size = "small"
	}
	variant := "secondary"
	switch p.Variant {
	case Primary:
		variant = "primary"
	case Destructive:
		variant = "danger"
	}
	if on {
		variant = "primary"
	}
	bp := boardui.ButtonProps{Variant: variant, Size: size, Label: p.Label, Disabled: p.Disabled, Busy: p.Busy, IconOnly: label == ""}
	if p.Icon != "" {
		bp.Leading = func(size float32) { k.Icon(c.Key("host-icon"), p.Icon, size).AccessHidden(true) }
	}
	if p.Variant == Link {
		e = k.native().DressLinkButton(c, e, label, bp)
	} else {
		e = k.native().DressButton(c, e, label, bp)
	}
	tip := p.Tooltip
	if tip == "" && label == "" {
		tip = p.Label
	}
	if tip != "" {
		e.Tooltip(tip)
	}
	return e
}
