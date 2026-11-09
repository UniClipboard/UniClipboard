package uikit

import (
	"github.com/egoist/mygo/ui"
)

// ToggleButtonProps mirrors contract.ts ToggleButtonProps; the pressed state is the *bool
// passed to ToggleButton.
type ToggleButtonProps struct {
	Size ControlSize
	Icon IconName
	// Label is the accessible name of an icon-only toggle.
	Label    string
	Tooltip  string
	Disabled bool
}

// ToggleButton is a button that stays pressed while *pressed; read changes with .Changed().
// label is the visible text ("" for an icon-only toggle named by p.Label).
func (k Kit) ToggleButton(c *ui.Context, pressed *bool, label string, p ToggleButtonProps) ui.Element {
	e := ui.ToggleBase(c, pressed)
	return k.dressButton(c, e, label, ButtonProps{Size: p.Size, Icon: p.Icon, Label: p.Label,
		Tooltip: p.Tooltip, Disabled: p.Disabled}, *pressed)
}

// CheckedControlProps mirrors contract.ts CheckedControlProps (Switch and Checkbox); the checked
// state is the *bool passed to the control.
type CheckedControlProps struct {
	// Label is the accessible name; shown next to the control unless LabelHidden.
	Label       string
	LabelHidden bool
	Disabled    bool
}

type (
	SwitchProps   = CheckedControlProps
	CheckboxProps = CheckedControlProps
)

// labelText draws the visible label of a switch or checkbox.
func (k Kit) labelText(c *ui.Context, s string) {
	size := k.baseFont()
	if k.Skin == Windows {
		size = k.Px("fontSizeBase300")
	}
	ui.Text(c, s).FontSize(size)
}

// Switch is an on/off switch toggling *on; read changes with .Changed().
func (k Kit) Switch(c *ui.Context, on *bool, p SwitchProps) ui.Element {
	var w, h, thumb, inset, travel float32
	switch k.Skin {
	case MacOS:
		w, h, thumb, inset, travel = 38, 22, 18, 2, 16
	case Windows:
		w, h, thumb, inset, travel = 40, 20, 12, 4, 20
	default:
		w, h, thumb, inset, travel = 48, 26, 20, 3, 22
	}
	e := ui.SwitchBase(c, on).Disabled(p.Disabled).Gap(10).Label(p.Label)
	x := float32(0)
	if *on {
		x = travel
	}
	x = e.AnimateTo("thumb", x, ui.Smooth)

	e.Children(func() {
		track := ui.Box(c).Size(w, h).Radius(h / 2)
		var thumbColor ui.Color
		switch k.Skin {
		case MacOS:
			track.Background(k.Color("fill-1"))
			thumbColor = k.Color("on-accent")
			if *on {
				track.Background(k.Color("accent"))
			}
		case Windows:
			thumbColor = k.Color("colorNeutralStrokeAccessible")
			track.Border(1, k.Color("colorNeutralStrokeAccessible"))
			if *on {
				track.Background(k.Color("colorBrandBackground")).Border(1, k.Color("colorBrandBackground"))
				thumbColor = k.Color("colorNeutralForegroundOnBrand")
			}
		default:
			thumbColor = k.Color("accent-fg")
			if *on {
				track.Background(k.Color("accent"))
			} else {
				track.Background(k.Color("btn")).InsetShadow(0, 0, 0, 1, k.Color("border"))
			}
		}
		track.Draw(func(pt *ui.Painter, r ui.Rect) {
			knob := ui.Rect{X: r.X + inset + x, Y: r.Y + (r.H-thumb)/2, W: thumb, H: thumb}
			if k.Skin != Windows {
				pt.Shadow(knob, thumb/2, 0, 1, 2, 0, ui.RGBA(0, 0, 0, .3))
			}
			pt.Fill(knob, thumbColor, thumb/2)
		})
		if !p.LabelHidden {
			k.labelText(c, p.Label)
		}
	})
	return e
}

// Checkbox is a check box toggling *checked; read changes with .Changed().
func (k Kit) Checkbox(c *ui.Context, checked *bool, p CheckboxProps) ui.Element {
	e := ui.CheckboxBase(c, checked).Disabled(p.Disabled).Label(p.Label).Gap(10)
	if k.Skin != Linux {
		e.Gap(8)
	}
	var size, radius float32
	switch k.Skin {
	case MacOS:
		size, radius = 16, 4
	case Windows:
		size, radius = 16, k.Px("borderRadiusSmall")
	default:
		size, radius = 20, 6
	}
	e.Children(func() {
		box := ui.Box(c).Size(size, size).Radius(radius).Center()
		fg := k.Color("accent-fg")
		switch k.Skin {
		case MacOS:
			box.Background(k.Color("ctrl"))
			cssApplyShadows(box, k.Raw("ctrl-shadow"))
			fg = k.Color("on-accent")
			if *checked {
				box.Background(k.Color("accent"))
			}
		case Windows:
			box.Border(1, k.Color("colorNeutralStrokeAccessible"))
			fg = k.Color("colorNeutralForegroundOnBrand")
			if *checked {
				box.Background(k.Color("colorCompoundBrandBackground")).Border(1, k.Color("colorCompoundBrandBackground"))
			}
		default:
			if *checked {
				box.Background(k.Color("accent"))
			} else {
				box.InsetShadow(0, 0, 0, 2, k.Color("border"))
			}
		}
		box.TextColor(fg)
		box.Children(func() {
			if *checked {
				k.glyph(c, "check", size-4)
			}
		})
		if !p.LabelHidden {
			k.labelText(c, p.Label)
		}
	})
	return e
}

// SliderProps mirrors contract.ts SliderProps; the value is the *float64 passed to Slider.
type SliderProps struct {
	// Label is the accessible name.
	Label    string
	Min, Max float64
	Step     float64 // 0 = a hundredth of the range
}

// Slider is a continuous value control; read changes with .Changed(). The default width is
// 100 DIPs (120 on Windows); override with .Width.
func (k Kit) Slider(c *ui.Context, value *float64, p SliderProps) ui.Element {
	var trackH, thumb, width float32
	switch k.Skin {
	case MacOS:
		trackH, thumb, width = 4, 18, 100
	case Windows:
		trackH, thumb, width = 4, 16, 120
	default:
		trackH, thumb, width = 6, 20, 100
	}
	e := ui.SliderBase(c, value, p.Min, p.Max).Label(p.Label).Width(width).Height(20).PaddingX(thumb / 2)
	if p.Step > 0 {
		e.Step(p.Step)
	}
	e.Draw(func(pt *ui.Painter, r ui.Rect) {
		x0, x1 := r.X+thumb/2, r.X+r.W-thumb/2
		frac := float32(0)
		if p.Max > p.Min {
			frac = float32((*value - p.Min) / (p.Max - p.Min))
		}
		frac = min(max(frac, 0), 1)
		cx := x0 + (x1-x0)*frac
		cy := r.Y + r.H/2
		track := ui.Rect{X: x0, Y: cy - trackH/2, W: x1 - x0, H: trackH}
		fill := ui.Rect{X: x0, Y: track.Y, W: cx - x0, H: trackH}
		knob := ui.Rect{X: cx - thumb/2, Y: cy - thumb/2, W: thumb, H: thumb}
		switch k.Skin {
		case MacOS:
			pt.Fill(track, k.Color("fill-1"), trackH/2)
			pt.Fill(fill, k.Color("accent"), trackH/2)
			pt.Shadow(knob, thumb/2, 0, 1, 3, 0, ui.RGBA(0, 0, 0, .2))
			pt.Shadow(knob, thumb/2, 0, .5, 1.5, 0, ui.RGBA(0, 0, 0, .2))
			pt.Fill(knob, ui.RGB(255, 255, 255), thumb/2)
		case Windows:
			pt.Fill(track, k.Color("colorNeutralStrokeAccessible"), trackH/2)
			pt.Fill(fill, k.Color("colorCompoundBrandBackground"), trackH/2)
			pt.Shadow(knob, thumb/2, 0, 1, 2, 0, ui.RGBA(0, 0, 0, .2))
			pt.Fill(knob, k.Color("colorNeutralBackground1"), thumb/2)
			dot := ui.Rect{X: cx - 4, Y: cy - 4, W: 8, H: 8}
			pt.Fill(dot, k.Color("colorCompoundBrandBackground"), 4)
		default:
			pt.Fill(track, k.Color("btn"), trackH/2)
			pt.Fill(fill, k.Color("accent"), trackH/2)
			pt.Shadow(knob, thumb/2, 0, 1, 3, 0, ui.RGBA(0, 0, 0, .3))
			pt.Fill(knob, ui.RGB(255, 255, 255), thumb/2)
			pt.Stroke(knob, k.Color("border"), thumb/2, 1)
		}
	})
	return e
}
