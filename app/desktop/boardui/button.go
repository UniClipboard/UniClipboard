package boardui

import (
	"time"

	"github.com/egoist/mygo/ui"
)

// ButtonProps preserves the fixed source's named variants and sizes. An empty
// label with Icon is icon-only. Label supplies its accessible name. Busy is a
// host state: it disables activation and exposes the accessibility busy flag.
type ButtonProps struct {
	Variant, Size             string
	Icon, TrailingIcon, Label string
	IconOnly                  bool
	Disabled, Busy            bool
}

// Button is a native mygo button with BoardUI paint and source press motion.
func (k Kit) Button(c *ui.Context, label string, p ButtonProps) ui.Element {
	size := normalizedSize(p.Size)
	variant := p.Variant
	if variant == "" {
		variant = "primary"
	}
	if variant != "primary" && variant != "secondary" && variant != "ghost" && variant != "danger" {
		panic("boardui: unknown button variant " + variant)
	}
	m := Geometry("button", size)
	disabled := p.Disabled || p.Busy
	iconOnly := p.IconOnly || (label == "" && p.Icon != "")
	name := p.Label
	if name == "" {
		name = label
	}
	if iconOnly && name == "" {
		panic("boardui: icon-only button requires Label")
	}
	e := ui.ButtonBase(c).Label(name).Disabled(disabled).DimDisabled(false).Busy(p.Busy).
		Height(m.Height).Padding(m.PaddingY, m.PaddingX).Radius(m.Radius).Gap(m.Gap)
	if iconOnly {
		e.Size(m.Height, m.Height).Padding(0)
	}
	style := "body-medium"
	if size == "xs" {
		style = "caption-1-semibold"
	}
	dressType(e, style)
	pressed := e.Pressed() || (e.Focused() && c.KeyDown(ui.KeySpace))
	state := "default"
	if e.Hovered() {
		state = "hover"
	}
	if pressed {
		state = "active"
	}
	if disabled {
		state = "disabled"
	}
	ink := Token(k.Dark, "text-white")
	switch variant {
	case "primary", "danger":
		from, to := Gradient(k.Dark, "button-"+variant+"-"+state)
		if state != "active" && !disabled && !c.Preferences().ReduceMotion {
			a := ui.Animation{Duration: time.Duration(Number("color-ms")) * time.Millisecond, Ease: ui.CubicBezier(.25, .1, .25, 1)}
			from = e.AnimateColor("gradient-start", from, a)
			to = e.AnimateColor("gradient-end", to, a)
		}
		e.Gradient(from, to, 180)
		if disabled {
			if variant == "primary" {
				ink = Token(k.Dark, "button-primary-disabled-foreground")
			} else {
				ink = Token(k.Dark, "foreground-disabled-danger")
			}
		}
	case "secondary":
		fill := "background-primary-default"
		edge := "border-button-default"
		if state == "hover" {
			fill = "background-primary-hover"
			edge = "border-button-hover"
		}
		if state == "active" {
			fill = "background-primary-active"
			edge = "border-button-active"
		}
		if disabled {
			fill = "background-primary-disabled"
		}
		e.Background(Token(k.Dark, fill)).Border(Number("button-secondary-border"), Token(k.Dark, edge))
		ink = Token(k.Dark, "text-primary")
		if disabled {
			ink = Token(k.Dark, "text-tertiary")
		}
	case "ghost":
		fill := "button-ghost-background"
		ink = Token(k.Dark, "button-ghost-foreground")
		if state == "hover" {
			fill = "button-ghost-hover"
		}
		if state == "active" {
			fill = "button-ghost-active"
		}
		if disabled {
			fill = "button-ghost-disabled"
			ink = Token(k.Dark, "button-ghost-disabled-foreground")
		}
		e.Background(Token(k.Dark, fill))
	}
	e.TextColor(ink)
	colorDuration := time.Duration(Number("color-ms")) * time.Millisecond
	if c.Preferences().ReduceMotion {
		colorDuration = -1
	}
	e.Transition(ui.ElementTransition{Duration: colorDuration, Colors: true, Shadows: true, Ease: ui.CubicBezier(.25, .1, .25, 1)})
	if !disabled && variant != "ghost" {
		k.shadows(e, "xs")
	}
	e.FocusRingColor(Token(k.Dark, "border-focus-ring")).FocusRingOffset(2)
	e.Children(func() {
		if p.Icon != "" {
			ui.Icon(c.Key("leading"), IconSVG(p.Icon)).Size(m.IconSize, m.IconSize).Shrink(0).TextColor(ink).AccessHidden(true)
		}
		if !iconOnly {
			pad := Number("button-label-padding-" + size)
			ui.Box(c.Key("label")).PaddingX(pad).Children(func() { k.Text(c.Key("text"), label, style).TextColor(ink).SingleLine() })
		}
		if !iconOnly && p.TrailingIcon != "" {
			ui.Icon(c.Key("trailing"), IconSVG(p.TrailingIcon)).Size(m.IconSize, m.IconSize).Shrink(0).TextColor(ink).AccessHidden(true)
		}
	})
	// Source theme.css button-press-motion, not the newer skill's no-scale recipe.
	scale := float32(1)
	duration := time.Duration(Number("return-ms")) * time.Millisecond
	if !disabled && pressed {
		scale = Number("press-scale")
		duration = time.Duration(Number("press-ms")) * time.Millisecond
	}
	if c.Preferences().ReduceMotion {
		scale = 1
	}
	e.AnimatePose(ui.Pose{Scale: scale}, ui.Animation{Duration: duration, Ease: ui.CubicBezier(.4, 0, .2, 1)})
	return e
}
