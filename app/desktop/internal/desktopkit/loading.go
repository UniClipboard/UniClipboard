package desktopkit

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// HighlightTextProps configures Kit.HighlightedText.
type HighlightTextProps struct {
	FontSize   float32
	FontWeight int
	TextColor  ui.Color
	Monospace  bool
	SingleLine bool
	MaxLines   int
	// OnDark selects higher-contrast highlight styling for dark surfaces such as code blocks.
	OnDark bool
}

// SearchHighlightColors returns the inline background and foreground colors used
// to mark matched search terms on normal or dark (code) surfaces.
func (k Kit) SearchHighlightColors(onDark bool) (bg, fg ui.Color) {
	if onDark {
		return k.Color("accent-500").Alpha(.32), ui.RGB(191, 219, 254)
	}
	if k.Dark {
		return k.Color("accent-500").Alpha(.24), k.Color("accent-500")
	}
	return k.Color("accent-500").Alpha(.18), k.Color("accent-600")
}

// HighlightedText draws text with matched search terms highlighted as rounded inline
// spans via ui.RichText so single-line truncation and multi-line wrapping stay intact.
func (k Kit) HighlightedText(c *ui.Context, text string, terms []string, p HighlightTextProps) ui.Element {
	col := p.TextColor
	if col == (ui.Color{}) {
		col = k.Color("text-primary")
	}
	runs, hit := highlightRuns(text, terms)
	var e ui.Element
	if len(runs) == 1 && !hit[0] {
		e = ui.Text(c, text).TextColor(col)
	} else {
		bg, fg := k.SearchHighlightColors(p.OnDark)
		spans := make([]ui.Span, len(runs))
		for i, r := range runs {
			sp := ui.Span{Text: r, Color: col}
			if hit[i] {
				sp.Background = bg
				sp.Color = fg
				sp.Radius = 3
				sp.PaddingX = 1.5
			}
			spans[i] = sp
		}
		e = ui.RichText(c, spans...).TextColor(col)
	}
	if p.FontSize > 0 {
		e.FontSize(p.FontSize)
	}
	if p.FontWeight > 0 {
		e.FontWeight(p.FontWeight)
	}
	if p.Monospace {
		e.Font("monospace")
	}
	if p.SingleLine {
		e.SingleLine()
	} else if p.MaxLines > 0 {
		e.MaxLines(p.MaxLines)
	}
	return e
}

// TopHairlineProgress draws a 2-DIP indeterminate accent progress hairline pinned to
// the bottom edge of its parent container (e.g. a toolbar or Quick Panel search row).
// Motion runs inside Painter.Draw so the view tree does not rebuild each frame.
func (k Kit) TopHairlineProgress(c *ui.Context, active bool) {
	bar := ui.Box(c).Absolute().Left(0).Right(0).Bottom(0).Height(2).Clip().ZIndex(10).AccessHidden(true)
	target := float32(0)
	if active {
		target = 1
	}
	alpha := bar.AnimateTo("hairline-a", target, ui.Animation{
		Duration:  160 * time.Millisecond,
		Ease:      ui.EaseOut,
		NonMotion: true,
	})
	if alpha <= 0.01 {
		return
	}
	accent := k.Color("accent-500")
	ease := ui.CubicBezier(.4, 0, .2, 1)
	bar.Draw(func(p *ui.Painter, r ui.Rect) {
		if r.W <= 0 || r.H <= 0 {
			return
		}
		if p.Preferences().ReduceMotion {
			p.Fill(r, accent.Alpha(.55*alpha), 1)
			return
		}
		const period = 1050 * time.Millisecond
		phase := float32(p.Now().UnixNano()%period.Nanoseconds()) / float32(period.Nanoseconds())
		eased := ease(phase)
		segW := r.W * 0.42
		x := r.X - segW + (r.W+segW)*eased
		seg := ui.Rect{X: x, Y: r.Y, W: segW, H: r.H}
		p.Clip(r, 0, func() {
			p.FillGradient(seg, ui.LinearGradient{
				Angle: 90,
				Stops: []ui.GradientStop{
					{Color: accent.Alpha(0), At: 0},
					{Color: accent.Alpha(.92 * alpha), At: 0.45},
					{Color: accent.Alpha(.92 * alpha), At: 0.65},
					{Color: accent.Alpha(0), At: 1},
				},
			}, r.H/2)
		})
		p.AnimationFrame()
	})
}

// ShimmerBox draws a calm BoardUI skeleton block with a low-contrast shimmer sweep.
// When ReduceMotion is enabled it renders a static tinted box with no animation loop.
func (k Kit) ShimmerBox(c *ui.Context, radius float32) ui.Element {
	base := k.Color("background-tertiary-default")
	hi := k.Color("background-primary-default").Alpha(.72)
	if k.Dark {
		base = ui.RGBA(255, 255, 255, .055)
		hi = ui.RGBA(255, 255, 255, .115)
	}
	box := ui.Box(c).Radius(radius).Clip().AccessHidden(true)
	box.Draw(func(p *ui.Painter, r ui.Rect) {
		if r.W <= 0 || r.H <= 0 {
			return
		}
		p.Fill(r, base, radius)
		if p.Preferences().ReduceMotion {
			return
		}
		const period = 1350 * time.Millisecond
		phase := float32(p.Now().UnixNano()%period.Nanoseconds()) / float32(period.Nanoseconds())
		bandW := r.W * 0.65
		x := r.X - bandW + (r.W+bandW)*phase
		band := ui.Rect{X: x, Y: r.Y, W: bandW, H: r.H}
		p.Clip(r, radius, func() {
			p.FillGradient(band, ui.LinearGradient{
				Angle: 90,
				Stops: []ui.GradientStop{
					{Color: hi.Alpha(0), At: 0},
					{Color: hi, At: 0.5},
					{Color: hi.Alpha(0), At: 1},
				},
			}, 0)
		})
		p.AnimationFrame()
	})
	return box
}
