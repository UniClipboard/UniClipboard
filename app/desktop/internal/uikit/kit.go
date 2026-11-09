// Package uikit is unic's native desktop component kit for mygo: one component API
// (ported from uniclipboard's ui-kit contract) with three skins — Adwaita (linux),
// AppKit glass (macos) and Fluent (windows) — whose design tokens are generated from
// uniclipboard's themes/default.json (see scripts/gen_tokens.py).
//
// A Kit is a small value: skin + colour scheme. Build one per view with For(dark) and
// call its methods; components never cache state in the Kit.
package uikit

import (
	"os"
	"runtime"

	"github.com/egoist/mygo/ui"
)

// SkinID names one of the three platform skins.
type SkinID string

const (
	Linux   SkinID = "linux"
	MacOS   SkinID = "macos"
	Windows SkinID = "windows"
)

// DefaultSkin picks the skin of the running OS; UNIC_UI_SKIN=linux|macos|windows overrides it
// (used to preview a skin on another OS).
func DefaultSkin() SkinID {
	switch SkinID(os.Getenv("UNIC_UI_SKIN")) {
	case Linux, MacOS, Windows:
		return SkinID(os.Getenv("UNIC_UI_SKIN"))
	}
	switch runtime.GOOS {
	case "darwin":
		return MacOS
	case "windows":
		return Windows
	}
	return Linux
}

// Kit is the component factory for one skin and colour scheme.
type Kit struct {
	Skin SkinID
	Dark bool
}

// For returns the kit of the running OS' skin.
func For(dark bool) Kit { return Kit{Skin: DefaultSkin(), Dark: dark} }

func (k Kit) scheme() string {
	if k.Dark {
		return "dark"
	}
	return "light"
}

// Raw returns a platform token as written in the CSS (e.g. "rgba(0, 0, 0, 0.8)", "8px"); "" if absent.
// Token names are the CSS custom properties without "--" and differ per skin.
func (k Kit) Raw(name string) string { return rawTokens[string(k.Skin)][k.scheme()][name] }

// Color returns a platform token parsed as a colour (transparent if absent or not a colour).
func (k Kit) Color(name string) ui.Color { c, _ := parseColor(k.Raw(name)); return c }

// Px returns a platform token such as "8px" as a number (0 if absent).
func (k Kit) Px(name string) float32 { return parsePx(k.Raw(name)) }

// Has reports whether the skin defines the token.
func (k Kit) Has(name string) bool { return k.Raw(name) != "" }

// Sem returns a semantic colour (see semantic.go for the names, which follow shadcn's:
// background, foreground, card, popover, primary, primaryForeground, secondary, muted,
// mutedForeground, accent, destructive, border, input, ring, chart1..5, sidebar*).
func (k Kit) Sem(name string) ui.Color { return k.Color(semanticTokens[k.Skin][name]) }

// Theme maps the skin onto mygo's theme so controls the kit does not wrap still match.
func (k Kit) Theme() *ui.Theme {
	t := ui.LightTheme()
	if k.Dark {
		t = ui.DarkTheme()
	}
	t.Background = k.Sem("background")
	t.Text, t.TextMuted = k.Sem("foreground"), k.Sem("mutedForeground")
	t.Surface, t.SurfaceHover = k.Sem("card"), k.Sem("secondary")
	t.SurfacePressed = k.Sem("muted")
	t.Border = k.Sem("border")
	t.Accent, t.AccentText = k.Sem("primary"), k.Sem("primaryForeground")
	t.AccentHover = t.Accent.Alpha(.88)
	t.AccentPressed = t.Accent.Alpha(.76)
	t.InactiveAccent = t.Accent
	t.Focus = k.Sem("ring")
	t.Danger, t.Warning, t.Success = k.Sem("destructive"), k.Sem("chart3"), k.Sem("chart2")
	t.Selection = t.Accent.Alpha(.25)
	t.Radius, t.FontSize, t.Spacing = k.ControlRadius(), 13, 4
	t.FocusRingWidth, t.FocusRingOffset = 2, 2
	return t
}

// Apply installs the theme on the view context; call it first thing in every view.
func (k Kit) Apply(c *ui.Context) {
	c.SetTheme(k.Theme())
	if k.Skin == MacOS {
		c.SetMenuPresentation(ui.SystemMenus)
	}
}

// ControlRadius is the corner radius of buttons and fields.
func (k Kit) ControlRadius() float32 {
	switch k.Skin {
	case MacOS:
		return 6
	case Windows:
		return 4
	}
	return 6
}
