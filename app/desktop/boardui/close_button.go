// Derived from official BoardUI components/base/buttons/close-button.tsx
// (commit ec5c5eb3e29c2441be63b9422cd54f411ef23466). MIT; see assets/BOARDUI-MIT.txt.
package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// CloseButtonProps configures the compact circular dismiss button ("2xs", "xs", "sm", "md").
// Label supplies the mandatory accessible name since there is no visible text.
type CloseButtonProps struct {
	Size     string
	Label    string
	Disabled bool
}

type closeButtonMetrics struct {
	container   float32
	glyph       float32
	strokeWidth float32
	inset       float32
}

var closeButtonSizes = map[string]closeButtonMetrics{
	"2xs": {container: 16, glyph: 6.8, strokeWidth: 1.6, inset: 0.57},
	"xs":  {container: 20, glyph: 10.8, strokeWidth: 2, inset: 2},
	"sm":  {container: 24, glyph: 12.6, strokeWidth: 2, inset: 2},
	"md":  {container: 32, glyph: 16.2, strokeWidth: 2.5, inset: 2},
}

// CloseButton is a native circular dismiss button with BoardUI's per-size X glyph.
func (k Kit) CloseButton(c *ui.Context, p CloseButtonProps) ui.Element {
	size := p.Size
	if size == "" {
		size = "xs"
	}
	m, ok := closeButtonSizes[size]
	if !ok {
		panic("boardui: unknown CloseButton size " + size)
	}
	if p.Label == "" {
		panic("boardui: CloseButton requires Label")
	}

	e := ui.ButtonBase(c).Label(p.Label).Disabled(p.Disabled).DimDisabled(false).
		Size(m.container, m.container).Shrink(0).Padding(0).Radius(m.container / 2).
		Justify(ui.Center).AlignItems(ui.Center).
		Background(k.Color("background-tertiary-default")).
		FocusRingColor(k.Color("border-focus-ring")).FocusRingOffset(2)

	ink := k.Color("foreground-icon-secondary")
	if !p.Disabled && (e.Hovered() || e.Pressed()) {
		ink = k.Color("text-primary")
	}
	if p.Disabled {
		e.Opacity(0.5)
	}

	colorDuration := time.Duration(Number("color-ms")) * time.Millisecond
	if c.Preferences().ReduceMotion {
		colorDuration = -1
	} else {
		ink = e.AnimateColor("close-ink", ink, ui.Animation{Duration: colorDuration, Ease: ui.CubicBezier(.25, .1, .25, 1)})
	}
	e.Transition(ui.ElementTransition{Duration: colorDuration, Colors: true, Ease: ui.CubicBezier(.25, .1, .25, 1)})
	e.TextColor(ink)

	e.Children(func() {
		glyph := ui.Box(c.Key("glyph")).Size(m.glyph, m.glyph).Shrink(0).AccessHidden(true)
		glyph.Draw(func(pt *ui.Painter, r ui.Rect) {
			x0, y0 := r.X+m.inset, r.Y+m.inset
			x1, y1 := r.X+m.glyph-m.inset, r.Y+m.glyph-m.inset
			p1 := (&ui.Path{}).Cap(ui.RoundCap).MoveTo(x0, y0).LineTo(x1, y1)
			p2 := (&ui.Path{}).Cap(ui.RoundCap).MoveTo(x1, y0).LineTo(x0, y1)
			pt.StrokePath(p1, m.strokeWidth, ink)
			pt.StrokePath(p2, m.strokeWidth, ink)
		})
	})
	return e
}
