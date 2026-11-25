package boardui

import (
	"fmt"
	"sync"
	"time"

	"github.com/egoist/mygo/ui"
)

// CloseButtonProps describes a compact circular dismiss control derived from
// BoardUI's CloseButton (components/base/buttons/close-button.tsx).
type CloseButtonProps struct {
	// Size selects one of "2xs", "xs" (default), "sm", or "md".
	Size string
	// Label is the required accessible name for the icon-only dismiss control.
	Label string
}

type closeButtonSpec struct {
	container float32
	glyph     float32
	stroke    float32
	inset     float32
}

var closeButtonSpecs = map[string]closeButtonSpec{
	"2xs": {container: 16, glyph: 6.8, stroke: 1.6, inset: 0.57},
	"xs":  {container: 20, glyph: 10.8, stroke: 2, inset: 2},
	"sm":  {container: 24, glyph: 12.6, stroke: 2, inset: 2},
	"md":  {container: 32, glyph: 16.2, stroke: 2.5, inset: 2},
}

var (
	closeGlyphOnce  sync.Once
	closeGlyphCache map[string]*ui.SVG
)

func closeButtonSVG(size string) *ui.SVG {
	closeGlyphOnce.Do(func() {
		closeGlyphCache = make(map[string]*ui.SVG, len(closeButtonSpecs))
		for name, sp := range closeButtonSpecs {
			end := sp.glyph - sp.inset
			src := fmt.Sprintf(
				`<svg width="%g" height="%g" viewBox="0 0 %g %g" fill="none" xmlns="http://www.w3.org/2000/svg">`+
					`<path d="M%g %gL%g %g" stroke="currentColor" stroke-width="%g" stroke-linecap="round"/>`+
					`<path d="M%g %gL%g %g" stroke="currentColor" stroke-width="%g" stroke-linecap="round"/>`+
					`</svg>`,
				sp.glyph, sp.glyph, sp.glyph, sp.glyph,
				sp.inset, sp.inset, end, end, sp.stroke,
				end, sp.inset, sp.inset, end, sp.stroke,
			)
			svg, err := ui.ParseSVG([]byte(src))
			if err != nil {
				panic("boardui: close button glyph " + name + ": " + err.Error())
			}
			closeGlyphCache[name] = svg
		}
	})
	return closeGlyphCache[size]
}

// CloseButton renders a circular dismiss button matching BoardUI CloseButton.
func (k Kit) CloseButton(c *ui.Context, p CloseButtonProps) ui.Element {
	size := p.Size
	if size == "" {
		size = "xs"
	}
	spec, ok := closeButtonSpecs[size]
	if !ok {
		panic("boardui: unknown close button size " + size)
	}
	if p.Label == "" {
		panic("boardui: CloseButton requires Label")
	}
	b := ui.ButtonBase(c).
		Label(p.Label).
		Size(spec.container, spec.container).
		Radius(spec.container / 2).
		Shrink(0).
		Center().
		Background(k.Color("background-tertiary-default")).
		FocusRingColor(k.Color("border-focus-ring")).
		FocusRingOffset(2)
	ink := k.Color("foreground-icon-secondary")
	if b.Hovered() || b.Pressed() {
		ink = k.Color("text-primary")
	}
	b.TextColor(ink)
	colorDuration := time.Duration(Number("color-ms")) * time.Millisecond
	if c.Preferences().ReduceMotion {
		colorDuration = -1
	}
	b.Transition(ui.ElementTransition{Duration: colorDuration, Colors: true, Ease: ui.CubicBezier(.25, .1, .25, 1)})
	b.Children(func() {
		ui.Icon(c.Key("x"), closeButtonSVG(size)).Size(spec.glyph, spec.glyph).TextColor(ink).AccessHidden(true)
	})
	return b
}
