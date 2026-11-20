package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// SelectItem has stable identity independent of its visible label.
type SelectItem struct {
	Value, Label string
	Disabled     bool
}

// SelectProps keeps the choice and open flag in the caller. A nil Open uses
// mygo's stable widget-local state. Value is required.
type SelectProps struct {
	Value                                        *string
	Open                                         *bool
	Items                                        []SelectItem
	Label, Description, Error, Placeholder, Size string
	Disabled                                     bool
}

// Select renders a non-modal native listbox. PopoverBase owns dismissal and
// focus restoration; FocusGroup owns arrows, Home/End and disabled skipping;
// ButtonBase owns Enter/Space activation. Typeahead is not supported by this
// first slice. Values and focus remain separate until the option is activated.
func (k Kit) Select(c *ui.Context, p SelectProps) ui.Element {
	if p.Value == nil {
		panic("boardui: Select requires Value")
	}
	size := p.Size
	if size == "" || size == "medium" {
		size = "md"
	}
	if size == "small" {
		size = "sm"
	}
	m := Geometry("select", size)
	var trigger ui.Element
	ui.Column(c).MinWidth(0).Gap(Number("select-popup-gap")).Children(func() {
		var label ui.Element
		if p.Label != "" {
			label = k.Text(c.Key("label"), p.Label, "body-medium")
		}
		trigger = ui.ButtonBase(c.Key("select")).Role(ui.RolePopUpButton).HasPopup(ui.PopupListBox).Label(p.Label).Disabled(p.Disabled).DimDisabled(false).Height(m.Height).Padding(m.PaddingY, m.PaddingX).Radius(m.Radius).Gap(m.Gap).MinWidth(0)
		if p.Label != "" {
			trigger.LabelledBy(label)
		}
		localOpen := ui.Local(trigger, "boardui-select-open", func() bool { return false })
		open := p.Open
		if open == nil {
			open = localOpen
		}
		if trigger.Clicked() && !p.Disabled {
			*open = !*open
		}
		if !*open && !p.Disabled && (trigger.Shortcut(0, ui.KeyDown) || trigger.Shortcut(0, ui.KeyUp)) {
			*open = true
		}
		style := "body-medium"
		if size == "sm" {
			style = "body-2-medium"
		}
		dressType(trigger, style)
		fill := "background-primary-default"
		edge := "border-button-default"
		ink := "text-primary"
		if trigger.Hovered() {
			fill = "background-primary-hover"
			edge = "border-button-hover"
		}
		if p.Disabled {
			fill = "background-primary-disabled"
			ink = "text-tertiary"
			*open = false
		}
		trigger.Background(Token(k.Dark, fill)).Border(m.BorderWidth, Token(k.Dark, edge)).TextColor(Token(k.Dark, ink)).Invalid(p.Error != "").FocusRingColor(Token(k.Dark, "border-focus-ring")).FocusRingOffset(2)
		if !p.Disabled {
			k.shadows(trigger, "xs")
		}
		text := p.Placeholder
		for _, it := range p.Items {
			if it.Value == *p.Value {
				text = it.Label
				break
			}
		}
		trigger.Value(text).Children(func() {
			k.Text(c.Key("value"), text, style).TextColor(Token(k.Dark, ink)).SingleLine().Grow(1).MinWidth(0)
			chevron := ui.Icon(c.Key("chevron"), IconSVG("chevron-down")).Size(m.IconSize, m.IconSize).Shrink(0).TextColor(Token(k.Dark, "text-secondary")).AccessHidden(true)
			angle := float32(0)
			if *open {
				angle = 180
			}
			chevron.AnimatePose(ui.Pose{Rotate: angle}, ui.Animation{Duration: time.Duration(Number("select-chevron-ms")) * time.Millisecond, Ease: ui.CubicBezier(Number("select-chevron-ease-x1"), Number("select-chevron-ease-y1"), Number("select-chevron-ease-x2"), Number("select-chevron-ease-y2"))})
		})
		if p.Description != "" {
			description := k.Text(c.Key("description"), p.Description, "caption-1-medium").TextColor(Token(k.Dark, "text-secondary"))
			trigger.DescribedBy(description)
		}
		if p.Error != "" {
			err := k.Text(c.Key("error"), p.Error, "caption-1-medium").TextColor(Token(k.Dark, "text-error-primary"))
			trigger.ErrorMessage(err)
		}
		trigger.Expanded(*open)
		width, height := c.Size()
		var initialFocus ui.Element
		panel := ui.PopoverBase(c, trigger, open, func(panel ui.Element) {
			panel.Role(ui.RoleListBox).KeepFocus().Label(p.Label).MinWidth(0).Width(min(Number("select-popup-width"), width-32)).MaxHeight(max(float32(32), height-32)).Padding(Number("select-popup-padding")).Gap(Number("select-popup-gap")).Radius(Number("select-popup-radius")).
				Margin(Number("select-popup-offset"), 0, 0).Border(1, Token(k.Dark, "border-button-default")).Background(Token(k.Dark, "background-primary-default")).AttachedOrigin()
			k.shadows(panel, "dropdown")
			panel.Transition(ui.ElementTransition{Duration: time.Duration(Number("popover-ms")) * time.Millisecond, Ease: ui.CubicBezier(Number("popover-ease-x1"), Number("popover-ease-y1"), Number("popover-ease-x2"), Number("popover-ease-y2")), Enter: &ui.Motion{Opacity: 0, Scale: .95, Blur: 2}, Exit: &ui.Motion{Opacity: 0, Scale: .95, Blur: 2}})
			ui.Scroll(c.Key("options")).MaxHeight(Number("select-popup-max-height")).Children(func() {
				ui.Column(c.Key("rows")).FocusGroup(ui.Vertical).Gap(Number("select-popup-gap")).Children(func() {
					for i, it := range p.Items {
						row := ui.ButtonBase(c.Key(it.Value)).Role(ui.RoleOption).Label(it.Label).Checked(it.Value == *p.Value).PositionInSet(i+1, len(p.Items)).TabStop(false).FocusRing(false).Disabled(it.Disabled).DimDisabled(false).Padding(Number("select-popup-row-padding")).Radius(Number("select-popup-row-radius")).Justify(ui.Start).MinWidth(0)
						if size == "sm" {
							row.Padding(Number("select-popup-row-padding-sm-y"), Number("select-popup-row-padding-sm-x"))
						}
						dressType(row, style)
						color := "text-primary"
						if it.Disabled {
							color = "text-disabled"
						}
						if !it.Disabled && (!initialFocus.Valid() || it.Value == *p.Value) {
							initialFocus = row
						}
						pointer := ui.Local(row, "boardui-select-pointer", func() selectPointer { return selectPointer{} })
						x, y, _ := row.PointerPosition()
						hovered := row.Hovered()
						if hovered && (!pointer.Hovered || pointer.X != x || pointer.Y != y) {
							row.Focus()
						}
						pointer.Hovered, pointer.X, pointer.Y = hovered, x, y
						if row.Focused() || row.Hovered() || it.Value == *p.Value {
							row.Background(Token(k.Dark, "dropdown-item-hover-background"))
						}
						row.Children(func() {
							k.Text(c.Key(it.Value), it.Label, style).SingleLine().TextColor(Token(k.Dark, color)).MinWidth(0)
						})
						if row.Clicked() && !it.Disabled {
							*p.Value = it.Value
							*open = false
							trigger.Focus()
						}
					}
				})
			})
		})
		// PopoverBase must record the trigger before programmatic option focus.
		initialFocus.AutoFocus()
		if panel.Valid() && panel.FocusLeft() {
			*open = false
		}
		trigger.Expanded(*open)
	})
	return trigger
}

type selectPointer struct {
	X, Y    float32
	Hovered bool
}
