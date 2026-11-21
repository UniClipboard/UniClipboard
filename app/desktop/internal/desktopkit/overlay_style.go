package desktopkit

import (
	"strconv"
	"strings"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// Application overlays share the pinned BoardUI surface and motion resources.
func (k Kit) shadowed(e ui.Element, name string) ui.Element {
	for _, s := range boardui.Shadows(k.Dark, name) {
		if s.Inset {
			e.InsetShadow(s.X, s.Y, s.Blur, s.Spread, s.Color)
		} else {
			e.Shadow(s.X, s.Y, s.Blur, s.Spread, s.Color)
		}
	}
	return e
}
func (k Kit) scrim() ui.Color {
	return ui.RGB(0, 0, 0).Alpha(boardui.Number("dialog-backdrop-opacity"))
}
func (k Kit) fgTone() (ui.Color, ui.Color, ui.Color) {
	return k.Color("text-primary"), k.Color("text-secondary"), k.Color("text-tertiary")
}
func (k Kit) dangerText() ui.Color            { return k.Color("text-error-primary") }
func (k Kit) hoverFill() ui.Color             { return k.Color("dropdown-item-hover-background") }
func (k Kit) dividerColor() ui.Color          { return k.Color("separator-border") }
func (k Kit) swatchColor(t TagColor) ui.Color { return k.SwatchColor(t) }
func (k Kit) popupSurface(panel ui.Element) ui.Element {
	panel.Padding(boardui.Number("select-popup-padding")).Radius(boardui.Number("select-popup-radius")).
		Background(k.Color("background-primary-default")).Border(1, k.Color("separator-border")).
		MinWidth(200).TextColor(k.Color("text-primary"))
	panel.AttachedOrigin().Transition(ui.ElementTransition{Duration: time.Duration(boardui.Number("popover-ms")) * time.Millisecond, Ease: ui.CubicBezier(0, 0, .2, 1), Colors: true, Enter: &ui.Motion{Scale: .95, Opacity: 0}})
	return k.shadowed(panel, "dropdown")
}
func (k Kit) dialogSurface(panel ui.Element) ui.Element {
	return k.shadowed(panel.Radius(boardui.Number("dialog-radius")).Background(k.Color("background-primary-default")).TextColor(k.Color("text-primary")), "dropdown")
}
func drawCheck(e ui.Element, col ui.Color, w float32) ui.Element {
	return e.Draw(func(p *ui.Painter, r ui.Rect) {
		var path ui.Path
		cx, cy := r.X+r.W/2, r.Y+r.H/2
		path.MoveTo(cx-4, cy)
		path.LineTo(cx-1, cy+3)
		path.LineTo(cx+4.5, cy-3.5)
		p.StrokePath(&path, w, col)
	})
}
func itoa(n int) string { return strconv.Itoa(n) }

// parseCustomColor remains for persisted user tag colors; no theme CSS is parsed.
func parseCustomColor(s string) (ui.Color, bool) {
	if !strings.HasPrefix(s, "#") {
		return ui.Color{}, false
	}
	return parseColor(s)
}
