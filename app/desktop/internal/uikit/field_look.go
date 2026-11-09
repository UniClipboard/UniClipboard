package uikit

import (
	"strings"

	"github.com/egoist/mygo/ui"
)

// fldPal collects the colours and metrics the input-family components share. Each skin
// fills it from its own tokens, so the components themselves only branch on structure.
type fldPal struct {
	bg, fg, fg2, hint ui.Color // field fill, text, secondary text, placeholder
	edge, underline   ui.Color // hairline edge, Windows rest underline
	focus, accent     ui.Color // focus ring (Adwaita/macOS), accent (macOS ring core, Fluent underline)
	danger            ui.Color
	fill              ui.Color // neutral chip / segmented-track fill
	hover, press      ui.Color // row hover and pressed fills
	popup             ui.Color // menu surface
	h, radius, padX   float32  // field height, corner radius, horizontal padding
	size              float32  // font size
	popupRadius       float32
}

func (k Kit) fldPal() fldPal {
	switch k.Skin {
	case MacOS:
		return fldPal{
			bg: k.Color("content"), fg: k.Color("label"), fg2: k.Color("label-2"), hint: k.Color("label-3"),
			edge: k.Color("separator"), underline: k.Color("separator"),
			focus: k.Color("focus"), accent: k.Color("accent"), danger: k.Color("red"),
			fill: k.Color("fill-2"), hover: k.Color("fill-4"), press: k.Color("fill-3"),
			popup: k.Color("popup-opaque"),
			h:     28, radius: 7, padX: 9, size: 13, popupRadius: 10,
		}
	case Windows:
		return fldPal{
			bg: k.Color("colorNeutralBackground1"), fg: k.Color("colorNeutralForeground1"),
			fg2: k.Color("colorNeutralForeground2"), hint: k.Color("colorNeutralForeground3"),
			edge: k.Color("colorNeutralStroke1"), underline: k.Color("colorNeutralStrokeAccessible"),
			focus: k.Color("colorStrokeFocus2"), accent: k.Color("colorCompoundBrandStroke"),
			danger: k.Color("colorPaletteRedForeground1"),
			fill:   k.Color("w11-selected"), hover: k.Color("colorSubtleBackgroundHover"),
			press: k.Color("colorSubtleBackgroundPressed"), popup: k.Color("colorNeutralBackground1"),
			h: 32, radius: 4, padX: 10, size: 14, popupRadius: 8,
		}
	}
	return fldPal{
		bg: k.Color("btn"), fg: k.Color("fg"), fg2: k.Color("fg2"), hint: k.Color("fg3"),
		edge: k.Color("border"), underline: k.Color("border"),
		focus: k.Color("focus"), accent: k.Color("accent"), danger: k.Color("destructive"),
		fill: k.Color("shade"), hover: k.Color("hover"), press: k.Color("press"),
		popup: k.Color("popover"),
		h:     34, radius: 6, padX: 10, size: 14, popupRadius: 12,
	}
}

// fldStyle draws the field chrome on e: fill, radius and the skin's focus / invalid
// indication (Adwaita 2px inner ring, macOS accent ring, Fluent accent underline).
func (k Kit) fldStyle(e ui.Element, p fldPal, focused, invalid, disabled bool) ui.Element {
	e.Radius(p.radius).Background(p.bg).TextColor(p.fg)
	switch k.Skin {
	case MacOS:
		e.InsetShadow(0, 0, 0, .5, p.edge)
		switch {
		case invalid && focused:
			e.Shadow(0, 0, 0, 3.5, p.danger.Alpha(.18))
		case invalid:
			e.Shadow(0, 0, 0, 1, p.danger)
		case focused:
			e.Shadow(0, 0, 0, 3.5, p.focus)
		default:
			e.Shadow(0, .5, 0, 0, p.press)
		}
	case Windows:
		e.Border(1, p.edge)
		switch {
		case invalid:
			e.InsetShadow(0, -2, 0, 0, p.danger)
		case focused:
			e.InsetShadow(0, -2, 0, 0, p.accent)
		default:
			e.InsetShadow(0, -1, 0, 0, p.underline)
		}
	default:
		switch {
		case invalid:
			e.InsetShadow(0, 0, 0, 2, p.danger)
		case focused:
			e.InsetShadow(0, 0, 0, 2, p.focus)
		}
	}
	if disabled {
		e.Opacity(.55)
	}
	return e
}

// fldBareInput makes a text input that sits inside a field chrome drawn by its parent.
func fldBareInput(e ui.Element, p fldPal, placeholder string) ui.Element {
	e.Background(ui.Transparent).TextColor(p.fg).FontSize(p.size).Grow(1).MinWidth(0).Height(p.h)
	if placeholder != "" {
		e.Placeholder(placeholder).PlaceholderColor(p.hint)
	}
	return e
}

// FieldResult is what text-like components report for one frame. Element is the outermost
// element (use it for layout: Grow, Width ...); Input is the text input itself (for
// Focus, Shortcut ...).
type FieldResult struct {
	Element, Input ui.Element
	Edited         bool // the text changed in this frame
	Submitted      bool // Return was pressed (not while an input method is composing)
	Escaped        bool // Escape was pressed in the field
}

// ---- tag / token colour families ------------------------------------------------------

// fldTagInk returns the chip fill and ink of a palette name ("orange","blue","green","purple",
// "gray"); a custom "#rrggbb" becomes a tinted chip, "" the neutral one.
func (k Kit) fldTagInk(color TagColor) (bg, ink ui.Color) {
	name := string(color)
	if strings.HasPrefix(name, "#") {
		if c, ok := parseColor(name); ok {
			return c.Alpha(.18), c
		}
		name = ""
	}
	switch name {
	case "orange", "blue", "green", "purple", "gray":
	default:
		name = "gray"
		if k.Skin == Windows {
			return k.Color("w11-tag-neutral-bg"), k.Color("w11-tag-neutral-ink")
		}
	}
	if k.Skin == Windows {
		return k.Color("w11-tag-" + name + "-bg"), k.Color("w11-tag-" + name + "-ink")
	}
	return k.Color("tag-" + name + "-bg"), k.Color("tag-" + name + "-ink")
}

// fldSwatch is the solid dot colour of a TagColor.
func (k Kit) fldSwatch(color TagColor) ui.Color {
	if strings.HasPrefix(string(color), "#") {
		if c, ok := parseColor(string(color)); ok {
			return c
		}
	}
	_, ink := k.fldTagInk(color)
	if k.Skin != Windows {
		if c := k.Color(string(color)); c.A != 0 {
			return c
		}
	}
	return ink
}

// SearchTokenKind hints the colour family of a search token (design: source green, tag
// orange, time blue). The zero value draws the accent tint.
type SearchTokenKind int

const (
	SearchKindDefault SearchTokenKind = iota
	SearchKindTag
	SearchKindSource
	SearchKindTime
	SearchKindType
	SearchKindOther
)

// fldKindColors returns the chip fill and ink for a token kind.
func (k Kit) fldKindColors(kind SearchTokenKind) (bg, ink ui.Color) {
	switch kind {
	case SearchKindTag:
		return k.fldTagInk("orange")
	case SearchKindSource:
		return k.fldTagInk("green")
	case SearchKindTime:
		return k.fldTagInk("blue")
	case SearchKindType, SearchKindOther:
		if k.Skin == Windows && kind == SearchKindType {
			return k.fldTagInk("purple")
		}
		if k.Skin == Linux {
			return k.Color("shade"), k.Color("fg")
		}
		return k.fldTagInk("gray")
	}
	switch k.Skin {
	case MacOS:
		return k.Color("accent-tint-18"), k.Color("accent")
	case Windows:
		return k.Color("colorBrandBackground2"), k.Color("colorBrandForeground2")
	}
	return k.Color("accent-tint-18"), k.Color("accent-text")
}

// fldChipMetrics returns height, radius and font size of search tokens and suggestion chips.
func (k Kit) fldChipMetrics() (h, radius, size float32) {
	switch k.Skin {
	case MacOS:
		return 20, 10, 12
	case Windows:
		return 24, 4, 14
	}
	return 22, 11, 12
}
