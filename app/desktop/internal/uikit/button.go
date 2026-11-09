package uikit

import (
	"github.com/egoist/mygo/ui"
)

// ButtonProps mirrors contract.ts ButtonProps. The label is the first argument of Button; an
// icon-only button passes "" there and names itself with Label.
type ButtonProps struct {
	Variant ButtonVariant
	Size    ControlSize
	Icon    IconName // optional leading icon
	// Label is the accessible name of an icon-only button; ignored when there is visible text.
	Label string
	// Tooltip is the hover/focus hint. For an icon-only button it defaults to Label.
	Tooltip string
	// Disabled blocks interaction and dims the button.
	Disabled bool
	// Busy marks work started by this button as running: the skin shows its progress glyph and
	// blocks clicks.
	Busy bool
}

// buttonMetrics are the geometry of one size step.
type buttonMetrics struct {
	h, padX, radius, font, icon float32
}

func (k Kit) buttonMetrics(size ControlSize) buttonMetrics {
	switch k.Skin {
	case MacOS:
		switch size {
		case SizeSmall:
			return buttonMetrics{20, 9, 6, 12, 14}
		case SizeLarge:
			return buttonMetrics{32, 16, 16, 13, 16}
		case SizeXLarge:
			return buttonMetrics{40, 20, 20, 14, 16}
		}
		return buttonMetrics{24, 12, 7, 13, 16}
	case Windows:
		switch size {
		case SizeSmall:
			return buttonMetrics{24, 8, 4, 12, 14}
		case SizeLarge, SizeXLarge: // Fluent stops at large
			return buttonMetrics{40, 16, 4, 14, 20}
		}
		return buttonMetrics{32, 12, 4, 14, 16}
	}
	switch size {
	case SizeSmall:
		return buttonMetrics{28, 10, 6, 13, 14}
	case SizeLarge:
		return buttonMetrics{40, 24, 999, 14, 16}
	case SizeXLarge:
		return buttonMetrics{46, 28, 999, 14, 16}
	}
	return buttonMetrics{34, 12, 6, 14, 16}
}

// Button is a push button; read the click with .Clicked(). label is the visible text; an
// icon-only button passes "" and sets ButtonProps.Label.
func (k Kit) Button(c *ui.Context, label string, p ButtonProps) ui.Element {
	return k.dressButton(c, ui.ButtonBase(c), label, p, false)
}

// dressButton gives a ButtonBase or ToggleBase element the look and content of a push button;
// on draws the toggled-on state of a ToggleButton.
func (k Kit) dressButton(c *ui.Context, e ui.Element, label string, p ButtonProps, on bool) ui.Element {
	m := k.buttonMetrics(p.Size)
	iconOnly := label == ""
	disabled := p.Disabled || p.Busy
	e.Disabled(disabled).Height(m.h).MinWidth(m.h).Gap(6).Radius(m.radius)
	if iconOnly {
		e.Padding(0).Width(m.h)
	} else {
		e.PaddingX(m.padX)
	}
	if p.Busy && !p.Disabled {
		e.DimDisabled(false)
	}
	name := label
	if iconOnly {
		name = p.Label
	}
	e.Label(name)
	tip := p.Tooltip
	if tip == "" && iconOnly {
		tip = p.Label
	}
	if tip != "" {
		e.Tooltip(tip)
	}

	state := visRest
	switch {
	case disabled:
	case e.Pressed() || on:
		state = visPressed
	case e.Hovered():
		state = visHover
	}
	weight := 400
	fg := k.styleButton(e, p.Variant, p.Size, state, disabled)
	if k.Skin == Linux {
		weight = 700
	}
	if k.Skin == MacOS && (p.Variant == Primary || p.Size == SizeXLarge) {
		weight = 600
	}
	e.TextColor(fg)
	// Glass already deforms its material on press; preserve that expansion.
	if p.Variant != Glass && p.Variant != Link {
		depth := float32(.97)
		if iconOnly || p.Variant == Subtle {
			depth = .92
		}
		k.pressFeedback(c, e, depth)
	}

	e.Children(func() {
		switch {
		case p.Busy:
			ui.Spinner(c).Size(m.icon, m.icon)
		case p.Icon != "":
			k.Icon(c, p.Icon, m.icon)
		}
		if !iconOnly {
			t := ui.Text(c, label).FontSize(m.font).FontWeight(weight)
			if p.Variant == Link && state == visHover && k.Skin != MacOS {
				t.Underline()
			}
		}
	})
	return e
}

type ctrlVisual int

const (
	visRest ctrlVisual = iota
	visHover
	visPressed
)

// styleButton paints the variant's look on e for the state and returns the text colour.
func (k Kit) styleButton(e ui.Element, v ButtonVariant, size ControlSize, st ctrlVisual, disabled bool) ui.Color {
	pick := func(rest, hover, press ui.Color) ui.Color {
		switch st {
		case visHover:
			return hover
		case visPressed:
			return press
		}
		return rest
	}
	switch k.Skin {
	case MacOS:
		return k.styleMacButton(e, v, st, disabled, pick)
	case Windows:
		return k.styleWinButton(e, v, st, disabled, pick)
	}
	// Linux: Adwaita.
	fg := k.Color("fg")
	switch v {
	case Primary:
		e.Background(pick(k.Color("accent"), k.Color("accent-hover"), k.Color("accent-pressed")))
		fg = k.Color("accent-fg")
	case Destructive:
		e.Background(pick(k.Color("destructive"), k.Color("destructive-hover"), k.Color("destructive-pressed")))
		fg = ui.RGB(255, 255, 255)
	case Subtle:
		e.Background(pick(ui.Transparent, k.Color("hover"), k.Color("press")))
	case Link:
		e.Height(0).MinWidth(0).PaddingY(2).PaddingX(6).Background(ui.Transparent)
		e.Height(0)
		fg = k.Color("accent-text")
		if disabled {
			fg = k.Color("fg3")
		}
	default:
		e.Background(pick(k.Color("btn"), k.Color("btn-hover"), k.Color("press")))
	}
	return fg
}

func (k Kit) styleMacButton(e ui.Element, v ButtonVariant, st ctrlVisual, disabled bool,
	pick func(rest, hover, press ui.Color) ui.Color) ui.Color {
	fg := k.Color("label")
	if disabled && v != Link {
		e.Background(k.Color("fill-2"))
		return k.Color("label-3")
	}
	switch v {
	case Glass:
		k.glassSurface(e)
	case Primary:
		e.Background(pick(k.Color("accent"), k.Color("accent"), k.Color("accent-pressed")))
		e.InsetShadow(0, 0.5, 0, 0, ui.RGBA(255, 255, 255, .25)).Shadow(0, 0.5, 1.5, 0, ui.RGBA(0, 0, 0, .2))
		fg = k.Color("on-accent")
	case Destructive:
		e.Background(pick(k.Color("red-tint-14"), k.Color("red-tint-14"), cssShade(k.Color("red-tint-14"), .08)))
		fg = k.Color("red")
	case Subtle:
		e.Background(pick(ui.Transparent, k.Color("fill-3"), k.Color("fill-2")))
	case Link:
		e.Height(0).MinWidth(0).PaddingY(2).PaddingX(6).Radius(5).Background(ui.Transparent)
		fg = k.Color("link")
		if disabled {
			fg = k.Color("label-3")
		}
	default:
		bg := pick(k.Color("ctrl"), k.Color("ctrl"), cssShade(k.Color("ctrl"), .08))
		e.Background(bg)
		cssApplyShadows(e, k.Raw("ctrl-shadow"))
	}
	return fg
}

func (k Kit) styleWinButton(e ui.Element, v ButtonVariant, st ctrlVisual, disabled bool,
	pick func(rest, hover, press ui.Color) ui.Color) ui.Color {
	fg := k.Color("colorNeutralForeground1")
	if disabled && v != Link {
		e.Background(k.Color("colorNeutralBackgroundDisabled")).Border(1, k.Color("colorNeutralStrokeDisabled"))
		if v == Subtle {
			e.Background(ui.Transparent).Border(0, ui.Transparent)
		}
		return k.Color("colorNeutralForegroundDisabled")
	}
	switch v {
	case Primary:
		e.Background(pick(k.Color("colorBrandBackground"), k.Color("colorBrandBackgroundHover"), k.Color("colorBrandBackgroundPressed")))
		e.Border(1, ui.Transparent)
		fg = k.Color("colorNeutralForegroundOnBrand")
	case Subtle:
		e.Background(pick(ui.Transparent, k.Color("colorSubtleBackgroundHover"), k.Color("colorSubtleBackgroundPressed")))
		fg = k.Color("colorNeutralForeground2")
	case Link:
		e.Height(0).MinWidth(0).PaddingY(2).PaddingX(k.Px("spacingHorizontalS")).Background(ui.Transparent)
		fg = pick(k.Color("colorBrandForegroundLink"), k.Color("colorBrandForegroundLinkHover"), k.Color("colorBrandForegroundLinkPressed"))
		if disabled {
			fg = k.Color("colorNeutralForegroundDisabled")
		}
	default: // Standard; Win11 has no destructive push button
		e.Background(pick(k.Color("colorNeutralBackground1"), k.Color("colorNeutralBackground1Hover"), k.Color("colorNeutralBackground1Pressed")))
		e.Border(1, pick(k.Color("colorNeutralStroke1"), k.Color("colorNeutralStroke1Hover"), k.Color("colorNeutralStroke1Pressed")))
		if st != visPressed {
			e.InsetShadow(0, -1, 0, 0, k.Color("w11-stroke-elevation"))
		}
	}
	return fg
}
