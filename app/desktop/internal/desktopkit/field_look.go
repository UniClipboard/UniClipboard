package desktopkit

import (
	"strings"

	"github.com/uniclipboard/unic/app/desktop/boardui"

	"github.com/egoist/mygo/ui"
)

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
	m := boardui.Geometry("input", "medium")
	return fldPal{
		bg: k.Color("background-tertiary-default"), fg: k.Color("text-primary"), fg2: k.Color("text-secondary"), hint: k.Color("text-tertiary"),
		edge: k.Color("border-button-default"), underline: k.Color("border-button-default"), focus: k.Color("border-button-active"), accent: k.Color("accent-500"), danger: k.Color("border-error-default"),
		fill: k.Color("background-secondary-default"), hover: k.Color("background-primary-hover"), press: k.Color("background-primary-active"), popup: k.Color("background-full"),
		h: m.Height, radius: m.Radius, padX: m.PaddingX, size: boardui.Typography("body-regular").Size, popupRadius: boardui.Number("select-popup-radius"),
	}
}

// fldStyle dresses application composites that contain a mature native editor.
func (k Kit) fldStyle(e ui.Element, p fldPal, focused, invalid, disabled bool) ui.Element {
	e.Radius(p.radius).Background(p.bg).TextColor(p.fg).FocusRing(false).DimDisabled(false)
	if invalid {
		e.Background(k.Color("background-tertiary-error"))
	}
	if disabled {
		return e.Background(k.Color("input-disabled-background")).TextColor(k.Color("input-disabled-text"))
	}
	edge := ui.Transparent
	if e.Hovered() {
		edge = k.Color("border-button-hover")
	}
	if focused {
		edge = p.focus
	}
	if invalid {
		edge = p.danger
	}
	if edge.A != 0 {
		e.InsetShadow(0, 0, 0, 2, edge)
	}
	return e
}

func fldBareInput(e ui.Element, p fldPal, placeholder string) ui.Element {
	t := boardui.Typography("body-regular")
	e.Background(ui.Transparent).TextColor(p.fg).Font(boardui.FontFamily).FontSize(t.Size).FixedLineHeight(t.LineHeight).FontWeight(t.Weight).LetterSpacing(t.LetterSpacing).Grow(1).MinWidth(0).Height(p.h)
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
	case "green":
		name = "lime"
	case "orange", "blue", "purple", "lime":
	default:
		return k.Color("background-secondary-default"), k.Color("text-primary")
	}
	return k.Color("status-" + name + "-background"), k.Color("status-" + name + "-text")
}

func (k Kit) fldSwatch(color TagColor) ui.Color {
	if strings.HasPrefix(string(color), "#") {
		if c, ok := parseColor(string(color)); ok {
			return c
		}
	}
	_, ink := k.fldTagInk(color)
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
	}
	return k.Color("background-secondary-default"), k.Color("text-primary")
}

// Caption chip geometry follows chip.tsx: py4, radius6, caption line-height16.
func (k Kit) fldChipMetrics() (h, radius, size float32) {
	t := boardui.Typography("caption-1-medium")
	return t.LineHeight + 8, 6, t.Size
}
