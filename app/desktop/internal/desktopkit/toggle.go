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
}

func (k Kit) Slider(c *ui.Context, value *float64, p SliderProps) ui.Element {
	step := p.Step
	if step <= 0 {
		step = (p.Max - p.Min) / 100
	}
	return k.native().Slider(c, value, boardui.SliderProps{Label: p.Label, HideLabel: true, Min: p.Min, Max: p.Max, Step: step})
}
