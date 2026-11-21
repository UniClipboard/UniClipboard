package desktopkit

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

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

// CheckedControlProps configures a checkbox; its checked state is passed by pointer.
type CheckedControlProps struct {
	// Label is the accessible name; shown next to the control unless LabelHidden.
	Label       string
	LabelHidden bool
	Disabled    bool
}

// SwitchProps configures an unlabeled visual switch. The enclosing row owns
// visible text. Label remains available to assistive technology.
type SwitchProps struct {
	Label    string
	Disabled bool
}

type CheckboxProps = CheckedControlProps

func (k Kit) Switch(c *ui.Context, on *bool, p SwitchProps) ui.Element {
	return k.native().Switch(c, on, boardui.SwitchProps{Label: p.Label, LabelHidden: true, Disabled: p.Disabled})
}

func (k Kit) Checkbox(c *ui.Context, checked *bool, p CheckboxProps) ui.Element {
	return k.native().Checkbox(c, checked, boardui.CheckboxProps{Label: p.Label, LabelHidden: p.LabelHidden, Disabled: p.Disabled})
}

type SliderProps struct {
	Label          string
	Min, Max, Step float64
	HideTooltip    bool
	// Compact draws a refined thin toolbar slider (4px track, 14px thumb, no tooltip bubble).
	Compact bool
}

func (k Kit) Slider(c *ui.Context, value *float64, p SliderProps) ui.Element {
	step := p.Step
	if step <= 0 {
		step = (p.Max - p.Min) / 100
	}
	if p.Compact {
		return k.compactSlider(c, value, p, step)
	}
	return k.native().Slider(c, value, boardui.SliderProps{Label: p.Label, HideLabel: true, HideTooltip: p.HideTooltip, Min: p.Min, Max: p.Max, Step: step})
}

func (k Kit) compactSlider(c *ui.Context, value *float64, p SliderProps, step float64) ui.Element {
	slider := ui.SliderBase(c.Key("slider"), value, p.Min, p.Max).
		Label(p.Label).Step(step).FocusRing(false).Height(28).Width(74).Padding(0)
	frac := float32(0)
	if p.Max > p.Min {
		frac = min(max(float32((*value-p.Min)/(p.Max-p.Min)), 0), 1)
	}
	dragging := slider.Pressed()
	slider.Children(func() {
		trackCol := k.Color("background-tertiary-default")
		if slider.Hovered() {
			trackCol = k.Color("background-tertiary-hover")
		}
		ui.Box(c.Key("track")).Absolute().Left(0).Right(0).Top(12).Height(4).Radius(2).
			Background(trackCol).AccessHidden(true)
		ui.Box(c.Key("fill")).Absolute().Left(0).Top(12).WidthPercent(frac*100).Height(4).Radius(2).
			Background(k.Color("accent-500")).AccessHidden(true)
		thumb := ui.Box(c.Key("thumb")).Absolute().LeftPercent(frac*100).Top(7).Translate(-7, 0).
			Size(14, 14).Radius(7).
			Background(k.Color("control-indicator-background")).
			Border(1.5, k.Color("accent-500")).
			Shadow(0, 1, 3, 0, ui.RGBA(0, 0, 0, .16)).
			AccessHidden(true)
		if dragging {
			thumb.Shadow(0, 2, 6, 0, k.Color("accent-500").Alpha(.32))
		}
	})
	return slider
}
