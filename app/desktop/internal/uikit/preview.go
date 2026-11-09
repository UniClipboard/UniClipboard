package uikit

import "github.com/egoist/mygo/ui"

// PreviewKind selects the frame around an item's content in a detail pane.
type PreviewKind int

const (
	PreviewPlain PreviewKind = iota
	PreviewCode              // dark card
	PreviewImage             // checkerboard behind transparent pixels
	PreviewFile              // grows with its content; the others have a bounded height and scroll
)

// PreviewFrameProps mirrors contract.ts PreviewFrameProps. TopEnd / BottomStart are builders
// for controls floating over two corners (design ME1: Fit / 100% top trailing, Quick Look
// bottom leading).
type PreviewFrameProps struct {
	Kind PreviewKind
	// Caption is a caption bar above the content, e.g. "3 lines · 74 characters";
	// CaptionTrailing is its display-only trailing text, e.g. a shortcut.
	Caption, CaptionTrailing string
	TopEnd, BottomStart      func()
}

// PreviewFrame frames the content built by body.
func (k Kit) PreviewFrame(c *ui.Context, p PreviewFrameProps, body func()) ui.Element {
	t := k.layTok()
	_, wh := c.Size()
	h := wh * .4
	if h < 160 {
		h = 160
	}
	if h > 448 {
		h = 448
	}
	e := ui.Column(c).Radius(k.layRadius()).Clip().Shrink(0).Background(t.Card)
	fg, fg2 := t.Fg, t.Fg2
	switch p.Kind {
	case PreviewCode:
		e.Background(t.Code)
		fg, fg2 = ui.RGBA(255, 255, 255, .88), ui.RGBA(255, 255, 255, .55)
	case PreviewImage:
		e.Background(t.View)
		if h < 260 {
			h = 260
		}
		shade := t.Shade
		e.Draw(func(pt *ui.Painter, r ui.Rect) {
			const s = 8 // 16 px checks, two per axis
			for y, row := r.Y, 0; y < r.Y+r.H; y, row = y+s, row+1 {
				for x, col := r.X, 0; x < r.X+r.W; x, col = x+s, col+1 {
					if (row+col)%2 == 0 {
						w, hh := float32(s), float32(s)
						if x+w > r.X+r.W {
							w = r.X + r.W - x
						}
						if y+hh > r.Y+r.H {
							hh = r.Y + r.H - y
						}
						pt.Fill(ui.Rect{X: x, Y: y, W: w, H: hh}, shade, 0)
					}
				}
			}
		})
	}
	if p.Kind != PreviewCode && k.Skin != MacOS {
		e.Border(1, t.CardStroke)
	} else if p.Kind != PreviewCode {
		e.InsetShadow(0, 0, 0, 0.5, t.CardStroke)
	}
	if p.Kind != PreviewFile {
		e.Height(h).MinHeight(160)
	}
	e.TextColor(fg)
	return e.Children(func() {
		if p.Caption != "" {
			ui.Row(c.Key("caption")).Padding(6, 12).Gap(8).AlignItems(ui.Center).Shrink(0).Children(func() {
				k.Text(c.Key("c"), p.Caption, TextProps{Role: RoleCaption}).TextColor(fg2)
				ui.Spacer(c)
				if p.CaptionTrailing != "" {
					k.Text(c.Key("k"), p.CaptionTrailing, TextProps{Role: RoleCaption}).TextColor(fg2)
				}
			})
		}
		inner := func() {
			if body != nil {
				body()
			}
		}
		if p.Kind == PreviewFile {
			ui.Column(c.Key("body")).Children(inner)
		} else {
			ui.Scroll(c.Key("body")).Grow(1).MinHeight(0).Children(inner)
		}
		if p.TopEnd != nil {
			ui.Row(c.Key("top-end")).Absolute().Top(10).Right(10).Gap(6).ZIndex(2).Children(p.TopEnd)
		}
		if p.BottomStart != nil {
			ui.Row(c.Key("bottom-start")).Absolute().Left(10).Bottom(10).Gap(6).ZIndex(2).Children(p.BottomStart)
		}
	})
}
