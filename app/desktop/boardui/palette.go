package boardui

import (
	"fmt"
	"math"
	"strings"

	"github.com/egoist/mygo/ui"
)

// Palette is the twelve-color authoring surface for one scheme. Zero colors
// inherit that scheme's defaults. Use WithOverrides for an exactly transparent token.
type Palette struct {
	Background, Surface, Elevated ui.Color
	Text, TextMuted, Border       ui.Color
	Accent, AccentText            ui.Color
	Success, Warning, Danger      ui.Color
	Overlay                       ui.Color
}

// DefaultPalette returns the source presentation's base colors for one scheme.
func DefaultPalette(dark bool) Palette {
	colors := defaultThemeColors[dark]
	return Palette{
		Background: colors["background-full"], Surface: colors["background-primary-default"], Elevated: colors["notification-center-background"],
		Text: colors["text-primary"], TextMuted: colors["text-secondary"], Border: colors["border-button-default"],
		Accent: colors["accent-500"], AccentText: colors["text-on-accent"],
		Success: colors["notification-success-foreground"], Warning: colors["status-yellow-text"], Danger: colors["text-error-primary"], Overlay: colors["dialog-backdrop"],
	}
}

// NewPaletteTheme derives component states from at most twelve colors per scheme.
// A one-color Accent palette updates all primary states. Empty palettes preserve
// every source token. The returned theme uses the same Kit and host APIs as raw tokens.
func NewPaletteTheme(light, dark Palette) *ColorTheme {
	return &ColorTheme{schemes: [2]map[string]ui.Color{derivePalette(light, false), derivePalette(dark, true)}}
}

// WithOverrides returns an immutable copy with exact final token replacements.
// Replacements do not re-derive other tokens. Set base colors on Palette instead.
func (t *ColorTheme) WithOverrides(light, dark map[string]ui.Color) (*ColorTheme, error) {
	result := &ColorTheme{}
	for i, overrides := range []map[string]ui.Color{light, dark} {
		colors := t.Tokens(i == 1)
		if err := applyColorOverrides(colors, overrides); err != nil {
			return nil, err
		}
		result.schemes[i] = colors
	}
	return result, nil
}

func resolvePalette(p, defaults Palette) Palette {
	fields := []*ui.Color{&p.Background, &p.Surface, &p.Elevated, &p.Text, &p.TextMuted, &p.Border, &p.Accent, &p.AccentText, &p.Success, &p.Warning, &p.Danger, &p.Overlay}
	values := []ui.Color{defaults.Background, defaults.Surface, defaults.Elevated, defaults.Text, defaults.TextMuted, defaults.Border, defaults.Accent, defaults.AccentText, defaults.Success, defaults.Warning, defaults.Danger, defaults.Overlay}
	for i, field := range fields {
		if *field == (ui.Color{}) {
			*field = values[i]
		}
	}
	return p
}

func derivePalette(input Palette, dark bool) map[string]ui.Color {
	colors := defaultColorTokens(dark)
	defaults := DefaultPalette(dark)
	p := resolvePalette(input, defaults)
	if p == defaults {
		return colors
	}
	set := func(color ui.Color, names ...string) {
		for _, name := range names {
			colors[name] = color
		}
	}
	neutral := p.Background != defaults.Background || p.Surface != defaults.Surface || p.Elevated != defaults.Elevated || p.Text != defaults.Text || p.TextMuted != defaults.TextMuted || p.Border != defaults.Border
	if neutral {
		hover := p.Surface.MixOklab(p.Text, .06)
		active := p.Surface.MixOklab(p.Text, .12)
		muted := p.TextMuted.MixOklab(p.Surface, .45)
		subtle := p.Surface.MixOklab(p.Text, .04)
		set(p.Background, "background-full", "segmented-control-background")
		set(p.Surface, "background-primary-default", "background-inner-default", "switch-off-chip-start", "switch-off-chip-end")
		set(p.Elevated, "notification-center-background", "segmented-control-selected-background", "background-dialog", "background-popover")
		set(hover, "background-primary-hover", "background-secondary-hover")
		set(p.Elevated.MixOklab(p.Text, .06), "dropdown-item-hover-background")
		set(active, "background-primary-active", "background-tertiary-default", "background-tertiary-hover", "badge-neutral-background")
		set(subtle, "background-primary-disabled", "background-secondary-default", "background-recent-hire-role", "input-disabled-background")
		set(p.Surface.MixOklab(p.Text, .20), "background-quaternary-default")
		set(p.Surface.MixOklab(p.Text, .30), "background-quaternary-hover")
		set(p.Text, "text-primary", "foreground-icon-primary", "foreground-icon-hover")
		set(p.TextMuted, "text-secondary", "foreground-icon-secondary")
		set(muted, "text-tertiary", "text-placeholder", "foreground-disabled", "foreground-icon-tertiary", "button-primary-disabled-foreground")
		set(muted.MixOklab(p.Surface, .35), "text-disabled", "foreground-icon-disabled", "foreground-icon-quaternary", "input-disabled-text", "input-disabled-foreground")
		set(p.Elevated, "foreground-full", "border-button-white", "border-checkbox-white")
		set(p.Border, "border-button-default", "border-button-group", "border-checkbox-default", "border-ai-profile-card", "border-component-detail-container", "border-table", "separator-border")
		set(p.Border.MixOklab(p.Text, .15), "border-button-hover", "border-checkbox-hover", "border-sidebar-profile-hover", "separator-border-strong")
		set(p.Border.MixOklab(p.Text, .30), "border-button-active", "border-checkbox-active")
		indicator := p.Elevated
		if dark {
			indicator = p.Text
		}
		set(indicator, "control-indicator-background")
		set(indicator.MixOklab(p.Surface, .08), "control-indicator-background-subtle")
		set(p.Elevated.Alpha(.95), "dock-background")
		set(p.Border, "dock-border")
		set(p.TextMuted.Alpha(.35), "native-scrollbar")
		for _, suffix := range []string{"from", "to"} {
			set(subtle, "gradient-button-primary-disabled-"+suffix)
		}
		// Named tag hues retain their meaning while their tint uses the new surface.
		for _, hue := range []string{"blue", "cyan", "lime", "orange", "purple", "rose", "yellow"} {
			ink := colors["status-"+hue+"-text"]
			set(p.Surface.MixOklab(ink, .14), "status-"+hue+"-background")
		}
	}
	if p.Accent != defaults.Accent || p.AccentText != defaults.AccentText {
		if input.AccentText == (ui.Color{}) {
			p.AccentText = readableInk(p.Accent.Over(p.Surface))
		}
		white, black := ui.RGB(255, 255, 255), ui.RGB(0, 0, 0)
		for _, stop := range []struct {
			name   string
			target ui.Color
			amount float32
		}{
			{"50", white, .94}, {"100", white, .86}, {"200", white, .72}, {"300", white, .52}, {"400", white, .24}, {"500", p.Accent, 0},
			{"600", black, .12}, {"700", black, .24}, {"800", black, .38}, {"900", black, .55}, {"950", black, .70},
		} {
			set(p.Accent.MixOklab(stop.target, stop.amount), "accent-"+stop.name)
		}
		set(p.Accent, "accent-500", "border-focus-ring", "native-inactive-accent", "shadow-checkbox-selected-1")
		set(p.AccentText, "text-on-accent", "checkbox-check")
		set(colors["accent-500"], "switch-on-chip-start")
		set(colors["accent-700"], "switch-on-chip-end")
		set(p.Accent.Alpha(.16), "slider-accent-shadow")
		set(p.Accent.Alpha(.25), "native-selection")
		for name, ends := range primaryGradientTokens {
			set(colors[ends[0]], "gradient-"+name+"-from")
			set(colors[ends[1]], "gradient-"+name+"-to")
		}
		tint := func(amount float32) ui.Color { return p.Surface.MixOklab(p.Accent, amount) }
		set(tint(.12), "button-ghost-background", "badge-new-background", "notification-information-background")
		set(tint(.20), "button-ghost-hover")
		set(tint(.30), "button-ghost-active")
		set(tint(.04), "button-ghost-disabled")
		set(p.Accent.MixOklab(p.Text, .25), "button-ghost-foreground", "badge-new-text", "notification-information-foreground")
		set(p.Accent.MixOklab(p.Surface, .65), "button-ghost-disabled-foreground")
	}
	if neutral { // Surface changes also affect accent tints without altering the ramp.
		if p.Accent == defaults.Accent && p.AccentText == defaults.AccentText {
			set(p.Surface.MixOklab(p.Accent, .12), "button-ghost-background", "badge-new-background", "notification-information-background")
			set(p.Surface.MixOklab(p.Accent, .20), "button-ghost-hover")
			set(p.Surface.MixOklab(p.Accent, .30), "button-ghost-active")
			set(p.Surface.MixOklab(p.Accent, .04), "button-ghost-disabled")
			set(p.Accent.MixOklab(p.Surface, .65), "button-ghost-disabled-foreground")
		}
	}
	if p.Success != defaults.Success || neutral {
		set(p.Success, "notification-success-foreground", "state-success-text", "status-lime-text")
		set(p.Surface.MixOklab(p.Success, .14), "notification-success-background", "state-success-base", "status-lime-background", "status-dot-green-halo")
	}
	if p.Warning != defaults.Warning || neutral {
		set(p.Warning, "status-yellow-text")
		set(p.Surface.MixOklab(p.Warning, .14), "status-yellow-background", "status-dot-yellow-halo")
	}
	if p.Danger != defaults.Danger || neutral {
		set(p.Danger, "text-error-primary", "foreground-icon-error", "border-error-default", "notification-error-foreground", "text-destructive")
		set(p.Danger.MixOklab(p.Surface, .45), "text-error-placeholder", "text-disabled-danger", "foreground-disabled-danger")
		set(p.Surface.MixOklab(p.Danger, .14), "background-tertiary-error", "notification-error-background")
		set(p.Surface.MixOklab(p.Danger, .25), "background-quaternary-error")
		set(readableInk(p.Danger.Over(p.Surface)), "text-on-danger")
		for _, state := range []struct {
			name     string
			from, to ui.Color
		}{
			{"default", p.Danger, p.Danger.MixOklab(p.Text, .12)},
			{"hover", p.Danger.MixOklab(p.Elevated, .20), p.Danger},
			{"active", p.Danger.MixOklab(p.Text, .12), p.Danger.MixOklab(p.Text, .24)},
			{"disabled", p.Surface.MixOklab(p.Danger, .12), p.Surface.MixOklab(p.Danger, .20)},
		} {
			set(state.from, "gradient-button-danger-"+state.name+"-from")
			set(state.to, "gradient-button-danger-"+state.name+"-to")
		}
	}
	if p.Overlay != defaults.Overlay || neutral {
		set(p.Overlay, "dialog-backdrop")
		for name, source := range colors {
			if strings.HasPrefix(name, "shadow-") || name == "slider-track-shadow" || name == "slider-thumb-shadow" || name == "switch-thumb-shadow" || name == "switch-thumb-edge" || name == "sidebar-selection-shadow" || name == "toggle-shadow" || name == "dock-shadow-far" || name == "dock-shadow-near" || name == "drawer-shadow-edge" || name == "drawer-shadow-inset" {
				if name == "shadow-checkbox-selected-0" || name == "shadow-checkbox-selected-1" {
					continue
				}
				shadow := p.Overlay
				shadow.A = source.A
				set(shadow, name)
			}
		}
	}
	return colors
}

// readableInk chooses black or white by the standard sRGB relative-luminance
// contrast formula. This is a foreground choice, not full-theme certification.
func readableInk(background ui.Color) ui.Color {
	linear := func(value uint8) float64 {
		v := float64(value) / 255
		if v <= .04045 {
			return v / 12.92
		}
		return math.Pow((v+.055)/1.055, 2.4)
	}
	luminance := .2126*linear(background.R) + .7152*linear(background.G) + .0722*linear(background.B)
	if (luminance+.05)/.05 >= 1.05/(luminance+.05) {
		return ui.RGB(0, 0, 0)
	}
	return ui.RGB(255, 255, 255)
}

func applyColorOverrides(colors, overrides map[string]ui.Color) error {
	for name, color := range overrides {
		if _, ok := colors[name]; !ok {
			return fmt.Errorf("boardui: unknown color token %q", name)
		}
		colors[name] = color
	}
	return nil
}
