package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// SwitchProps implements the pinned source md track. Shape is pill or rectangle.
// LabelHidden keeps Label accessible while the application owns visible copy.
type SwitchProps struct {
	Label, Shape          string
	LabelHidden, Disabled bool
}

// Switch uses SwitchBase for pointer, Space, focus and checked accessibility.
func (k Kit) Switch(c *ui.Context, value *bool, p SwitchProps) ui.Element {
	if value == nil {
		panic("boardui: Switch requires Value")
	}
	if p.Shape != "" && p.Shape != "pill" && p.Shape != "rectangle" {
		panic("boardui: unsupported Switch shape")
	}
	e := ui.SwitchBase(c, value).Label(p.Label).Disabled(p.Disabled).DimDisabled(false).FocusRing(false).AlignItems(ui.Center).Gap(8)
	e.Children(func() {
		// Exact md literals from switch.tsx at ec5c5eb3: 42x24, thumb18, pad3, chip7.5.
		trackRadius, thumbRadius, chipRadius := float32(12), float32(9), float32(3.75)
		if p.Shape == "rectangle" {
			trackRadius, thumbRadius, chipRadius = 4.5, 1.5, .75
		}
		track := ui.Box(c.Key("track")).Size(42, 24).Shrink(0).Radius(trackRadius)
		if *value {
			track.Gradient(Token(k.Dark, "accent-500"), Token(k.Dark, "accent-600"), 180).
				InsetShadow(0, 1.5, 0, 0, ui.RGBA(255, 255, 255, .25)).InsetShadow(0, 0, 0, .75, Token(k.Dark, "accent-500"))
		} else {
			track.Background(Token(k.Dark, "background-tertiary-default"))
		}
		if p.Disabled {
			track.Opacity(.5)
		}
		if e.FocusVisible() {
			track.Draw(func(p *ui.Painter, r ui.Rect) {
				p.FocusRing(r, [4]float32{trackRadius, trackRadius, trackRadius, trackRadius})
			})
		}
		x := float32(3)
		if *value {
			x += 18
		}
		x = track.AnimateTo("thumb-x", x, ui.Animation{Duration: 200 * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1)})
		track.Children(func() {
			thumb := ui.Box(c.Key("thumb")).Absolute().Left(x).Top(3).Size(18, 18).Radius(thumbRadius).Center().
				LinearGradient(ui.LinearGradient{From: Token(k.Dark, "control-indicator-background"), To: Token(k.Dark, "control-indicator-background-subtle"), Start: .43837, End: 1, Angle: 180, Oklab: true}).
				Shadow(0, 3, 3, 0, ui.RGBA(0, 0, 0, .03)).Shadow(0, .75, 0, 0, ui.RGBA(0, 0, 0, .05))
			thumb.Children(func() {
				start, end, edge := "switch-off-chip-start", "switch-off-chip-end", Token(k.Dark, "border-button-default").Alpha(.5)
				if *value {
					start, end, edge = "switch-on-chip-start", "switch-on-chip-end", Token(k.Dark, "accent-600")
				}
				ui.Box(c.Key("chip")).Size(7.5, 7.5).Radius(chipRadius).Border(.375, edge).
					LinearGradient(ui.LinearGradient{From: Token(k.Dark, start), To: Token(k.Dark, end), Start: .43837, End: 1, Angle: 0, Oklab: true}).Shadow(0, 3, 3, 0, ui.RGBA(0, 0, 0, .03)).AccessHidden(true)
			})
		})
		if !p.LabelHidden && p.Label != "" {
			k.Text(c.Key("label"), p.Label, "body-medium")
		}
	})
	return e
}
