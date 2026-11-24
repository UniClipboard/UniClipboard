package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// CheckboxProps selects the source md/sm checkbox. Label can remain accessible
// without a visible duplicate. Mixed values use CheckboxState.
type CheckboxProps struct {
	Label, Size           string
	LabelHidden, Disabled bool
}

func (k Kit) Checkbox(c *ui.Context, value *bool, p CheckboxProps) ui.Element {
	if value == nil {
		panic("boardui: Checkbox requires Value")
	}
	e := ui.CheckboxBase(c, value)
	return k.dressCheckbox(c, e, *value, false, p)
}

// CheckboxState reuses mygo's three-state toggle model and mixed accessibility.
func (k Kit) CheckboxState(c *ui.Context, value *ui.CheckState, p CheckboxProps) ui.Element {
	if value == nil {
		panic("boardui: CheckboxState requires Value")
	}
	e := ui.CheckboxStateBase(c, value)
	return k.dressCheckbox(c, e, *value == ui.CheckOn, *value == ui.CheckMixed, p)
}

func (k Kit) dressCheckbox(c *ui.Context, e ui.Element, checked, mixed bool, p CheckboxProps) ui.Element {
	size, gap, style := float32(16), float32(8), "body-medium"
	switch p.Size {
	case "", "md", "medium":
	case "sm", "small":
		size, gap, style = 14, 6, "body-2-medium"
	default:
		panic("boardui: unsupported Checkbox size")
	}
	e.Label(p.Label).Disabled(p.Disabled).DimDisabled(false).FocusRing(false).AlignItems(ui.Center).Gap(gap)
	marked := checked || mixed
	e.Children(func() {
		box := ui.Box(c.Key("box")).Size(size, size).Shrink(0).Radius(4)
		if marked {
			from, to := "accent-500", "accent-600"
			if e.Hovered() && !p.Disabled {
				from, to = "accent-400", "accent-500"
			}
			box.Gradient(k.Color(from), k.Color(to), 180).
				InsetShadow(0, 2, 0, 0, k.Color("control-highlight")).InsetShadow(0, 0, 0, 1, k.Color("accent-500"))
		} else {
			edge := "border-checkbox-default"
			if e.Hovered() && !p.Disabled {
				edge = "border-checkbox-hover"
			}
			box.Background(k.Color("background-primary-default")).Border(1, k.Color(edge))
			k.shadows(box, "xs")
		}
		if p.Disabled {
			box.Opacity(.5)
		}
		box.Draw(func(pt *ui.Painter, r ui.Rect) {
			if e.FocusVisible() {
				pt.FocusRing(r, [4]float32{4, 4, 4, 4})
			}
		})
		if marked {
			box.Children(func() {
				glyph := ui.Box(c.Key("glyph")).Absolute().Left(0).Top(0).Size(size, size).AccessHidden(true)
				progress := float32(1)
				if !mixed {
					progress = glyph.Keyframes("check-draw", []ui.Keyframe{{At: 0, Value: 0}, {At: 200 * time.Millisecond, Value: 1, Ease: ui.CubicBezier(.65, 0, .35, 1)}})
				}
				glyph.Draw(func(pt *ui.Painter, r ui.Rect) {
					// Exact checkbox-glyph.tsx path in its 16-unit source viewBox.
					scale := size / 16
					x := func(v float32) float32 { return r.X + v*scale }
					y := func(v float32) float32 { return r.Y + v*scale }
					path := (&ui.Path{}).Cap(ui.RoundCap).Join(ui.RoundJoin)
					if mixed {
						path.MoveTo(x(4.5), y(8)).LineTo(x(8), y(8)).LineTo(x(11.5), y(8))
					} else {
						path.MoveTo(x(4), y(7.7002)).LineTo(x(6.64645), y(10.3466)).CubeTo(x(6.84171), y(10.5419), x(7.15829), y(10.5419), x(7.35355), y(10.3466)).LineTo(x(12), y(5.7002)).Trim(0, progress)
					}
					pt.StrokePath(path, 2*scale, k.Color("checkbox-check"))
				})
			})
		}
		if !p.LabelHidden && p.Label != "" {
			k.Text(c.Key("label"), p.Label, style)
		}
	})
	return e
}
