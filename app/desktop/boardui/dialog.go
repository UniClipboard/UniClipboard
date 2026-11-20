package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// DialogProps describes a bounded settings form shell. It is derived from the
// pinned Settings Modal surface, not the unavailable newer generic Modal.
type DialogProps struct {
	Open               *bool
	Title, Description string
	Width              float32
}

// Dialog uses native mygo modal focus scopes, top-overlay Escape and opener
// restoration. The body follows the title/description and owns its own form
// state and actions. It returns the panel or a zero element while closed.
func (k Kit) Dialog(c *ui.Context, p DialogProps, body func()) ui.Element {
	if p.Open == nil {
		panic("boardui: Dialog requires Open")
	}
	w, h := c.Size()
	width := p.Width
	if width == 0 {
		width = 480
	}
	panel := ui.DialogBase(c, p.Open, func(backdrop, panel ui.Element) {
		backdrop.Background(ui.RGBA(0, 0, 0, Number("dialog-backdrop-opacity")))
		panel.Label(p.Title).Description(p.Description).TabStop(false).FocusRing(false).Column().Width(min(width, w-32)).MaxHeight(h - 32).Padding(Number("dialog-padding")).Gap(Number("dialog-header-body-gap")).
			Radius(Number("dialog-radius")).Background(Token(k.Dark, "background-full"))
		k.shadows(panel, "xs")
		panel.Transition(ui.ElementTransition{Duration: time.Duration(Number("dialog-ms")) * time.Millisecond, Ease: ui.CubicBezier(.32, .72, 0, 1), Enter: &ui.Motion{Opacity: 0, Scale: .85, Blur: 4}, Exit: &ui.Motion{Opacity: 0, Scale: .85, Blur: 4}})
		panel.Children(func() {
			ui.Column(c.Key("heading")).Gap(4).Children(func() {
				k.Text(c.Key("title"), p.Title, "title-2-medium").Role(ui.RoleHeading).Level(2)
				if p.Description != "" {
					k.Text(c.Key("description"), p.Description, "body-regular").TextColor(Token(k.Dark, "text-secondary"))
				}
			})
			if body != nil {
				body()
			}
		})
	})
	// DialogBase records its opener after its body callback returns. Focusing
	// inside that callback would record the new panel as its own opener.
	return panel.AutoFocus()
}
