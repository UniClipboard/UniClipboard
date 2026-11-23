package boardui

import (
	"fmt"
	"maps"
	"slices"
	"strings"

	"github.com/egoist/mygo/ui"
)

// ColorTheme contains immutable resolved light/dark colors. Its zero value and
// a nil pointer select the source palette. Construct overrides once, outside View.
type ColorTheme struct{ schemes [2]map[string]ui.Color }

// NewColorTheme overlays code-defined colors on the current presentation.
// Unknown names return an error. Input maps are copied and never retained.
// Partial overrides inherit the default palette, not the opposite scheme.
func NewColorTheme(light, dark map[string]ui.Color) (*ColorTheme, error) {
	theme := &ColorTheme{}
	for i, overrides := range []map[string]ui.Color{light, dark} {
		colors := defaultColorTokens(i == 1)
		for _, name := range slices.Sorted(maps.Keys(overrides)) {
			if _, ok := colors[name]; !ok {
				return nil, fmt.Errorf("boardui: unknown color token %q", name)
			}
			colors[name] = overrides[name]
		}
		// Primary gradients share the accent ramp unless explicitly overridden.
		for name, ends := range primaryGradientTokens {
			for j, end := range []string{"from", "to"} {
				token := "gradient-" + name + "-" + end
				if _, explicit := overrides[token]; !explicit {
					colors[token] = colors[ends[j]]
				}
			}
		}
		if _, explicit := overrides["shadow-checkbox-selected-1"]; !explicit {
			colors["shadow-checkbox-selected-1"] = colors["accent-500"]
		}
		for name, color := range nativeDerivedColors(colors) {
			if _, explicit := overrides[name]; !explicit {
				colors[name] = color
			}
		}
		theme.schemes[i] = colors
	}
	return theme, nil
}

// Tokens returns an independent snapshot suitable for inspection or export.
func (t *ColorTheme) Tokens(dark bool) map[string]ui.Color {
	if colors := t.scheme(dark); colors != nil {
		return maps.Clone(colors)
	}
	return defaultColorTokens(dark)
}

func (t *ColorTheme) scheme(dark bool) map[string]ui.Color {
	if t == nil {
		return nil
	}
	if dark {
		return t.schemes[1]
	}
	return t.schemes[0]
}

// Color resolves one theme token, accepting the source's optional CSS prefix.
func (t *ColorTheme) Color(dark bool, name string) ui.Color {
	name = strings.TrimPrefix(name, "--color-")
	if colors := t.scheme(dark); colors != nil {
		if color, ok := colors[name]; ok {
			return color
		}
	} else if color, ok := defaultThemeColors[dark][name]; ok {
		return color
	}
	panic("boardui: unknown color token " + name)
}

// ColorTokenNames lists the canonical token vocabulary in a stable order.
func ColorTokenNames() []string { return slices.Sorted(maps.Keys(defaultThemeColors[false])) }

var primaryGradientTokens = map[string][2]string{
	"button-primary-default": {"accent-500", "accent-600"},
	"button-primary-hover":   {"accent-400", "accent-500"},
	"button-primary-active":  {"accent-600", "accent-700"},
}

var defaultThemeColors = map[bool]map[string]ui.Color{
	false: defaultColorTokens(false), true: defaultColorTokens(true),
}

func defaultColorTokens(dark bool) map[string]ui.Color {
	colors := maps.Clone(generatedColors[dark])
	for name, ends := range generatedGradients[dark] {
		colors["gradient-"+name+"-from"] = ends[0]
		colors["gradient-"+name+"-to"] = ends[1]
	}
	for name, layers := range generatedShadows[dark] {
		for i, layer := range layers {
			colors[fmt.Sprintf("shadow-%s-%d", name, i)] = layer.Color
		}
	}
	for name, color := range presentationColors {
		colors[name] = color
	}
	colors["background-dialog"] = colors["background-full"]
	colors["background-popover"] = colors["background-primary-default"]
	colors["text-on-accent"] = colors["text-white"]
	colors["text-on-danger"] = colors["text-white"]
	colors["dialog-backdrop"] = ui.RGBA(0, 0, 0, Number("dialog-backdrop-opacity"))
	colors["dock-background"] = colors["background-primary-default"].Alpha(.95)
	colors["dock-border"] = ui.RGBA(10, 10, 10, .09)
	colors["dock-shadow-far"] = ui.RGBA(0, 0, 0, .10)
	colors["dock-shadow-near"] = ui.RGBA(0, 0, 0, .06)
	colors["drawer-shadow-inset"] = ui.RGBA(0, 0, 0, .16)
	colors["drawer-shadow-edge"] = ui.RGBA(0, 0, 0, .12)
	if dark {
		colors["dock-background"] = ui.RGBA(34, 34, 34, .94)
		colors["dock-border"] = ui.RGBA(255, 255, 255, .10)
		colors["dock-shadow-far"] = ui.RGBA(0, 0, 0, .48)
		colors["dock-shadow-near"] = ui.RGBA(0, 0, 0, .32)
		colors["drawer-shadow-inset"] = ui.RGBA(0, 0, 0, .58)
		colors["drawer-shadow-edge"] = ui.RGBA(0, 0, 0, .45)
	}
	for name, color := range nativeDerivedColors(colors) {
		colors[name] = color
	}
	return colors
}

// These existing native paints supplement, rather than modify, generated source resources.
var presentationColors = map[string]ui.Color{
	"control-highlight":        ui.RGBA(255, 255, 255, .25),
	"slider-highlight":         ui.RGBA(255, 255, 255, .24),
	"slider-thumb-highlight":   ui.RGBA(255, 255, 255, .8),
	"slider-track-shadow":      ui.RGBA(0, 0, 0, .06),
	"slider-accent-shadow":     ui.RGBA(4, 80, 226, .16),
	"slider-thumb-shadow":      ui.RGBA(0, 0, 0, .1),
	"switch-thumb-shadow":      ui.RGBA(0, 0, 0, .03),
	"switch-thumb-edge":        ui.RGBA(0, 0, 0, .05),
	"checkbox-check":           ui.RGB(255, 255, 255),
	"sidebar-selection-shadow": ui.RGBA(0, 0, 0, .04),
	"toggle-shadow":            ui.RGBA(0, 0, 0, .16),
}

// Color resolves the active scheme from this Kit's immutable theme.
func (k Kit) Color(name string) ui.Color { return k.Colors.Color(k.Dark, name) }

// Gradient resolves independently customizable endpoints from the same palette.
func (k Kit) Gradient(name string) (ui.Color, ui.Color) {
	return k.Color("gradient-" + name + "-from"), k.Color("gradient-" + name + "-to")
}

// Shadows preserves source geometry and resolves each layer's theme color.
func (k Kit) Shadows(name string) []Shadow {
	layers := Shadows(k.Dark, name)
	for i := range layers {
		layers[i].Color = k.Color(fmt.Sprintf("shadow-%s-%d", name, i))
	}
	return layers
}

func nativeDerivedColors(colors map[string]ui.Color) map[string]ui.Color {
	return map[string]ui.Color{
		"native-inactive-accent": colors["accent-500"],
		"native-selection":       colors["accent-500"].Alpha(.25),
		"native-scrollbar":       colors["text-secondary"].Alpha(.35),
		"native-inverse":         {},
		"native-inverse-text":    {},
	}
}
