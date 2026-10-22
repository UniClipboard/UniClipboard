package main

import (
	"io"
	"math"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"strconv"

	"github.com/BurntSushi/toml"
)

var omarchyHexColor = regexp.MustCompile(`^#[0-9a-fA-F]{6}$`)

func readOmarchyTheme() (*DesktopTheme, bool) {
	if runtime.GOOS != "linux" {
		return nil, false
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return nil, false
	}
	// Omarchy moved the active theme from config to state. Prefer the current
	// location, retaining support for installations using the previous layout.
	paths := []string{filepath.Join(home, ".local/state/omarchy/current/theme/colors.toml")}
	config, err := os.UserConfigDir()
	if err == nil {
		paths = append(paths, filepath.Join(config, "omarchy/current/theme/colors.toml"))
	}
	for _, path := range paths {
		f, err := os.Open(path)
		if os.IsNotExist(err) {
			continue
		}
		if err != nil {
			return nil, false
		}
		raw, err := io.ReadAll(io.LimitReader(f, 65537))
		f.Close()
		if err != nil || len(raw) > 65536 {
			return nil, false
		}
		var colors map[string]string
		if toml.Unmarshal(raw, &colors) != nil {
			return nil, false
		}
		theme := omarchyPalette(colors)
		return theme, theme != nil
	}
	return nil, false
}

func omarchyPalette(colors map[string]string) *DesktopTheme {
	for _, key := range []string{"background", "foreground", "accent"} {
		if !omarchyHexColor.MatchString(colors[key]) {
			return nil
		}
	}
	color := func(key, fallback string) string {
		if value := colors[key]; omarchyHexColor.MatchString(value) {
			return value
		}
		return fallback
	}
	bg, fg, primary := colors["background"], colors["foreground"], colors["accent"]
	surface := color("lighter_background", bg)
	selection := color("selection", surface)
	border := color("muted", selection)
	destructive := color("red", primary)
	dark := relativeLuminance(bg) < relativeLuminance(fg)
	if mode := colors["mode"]; mode == "light" || mode == "dark" {
		dark = mode == "dark"
	}
	return &DesktopTheme{Dark: dark, Variables: map[string]string{
		"--background": bg, "--foreground": fg,
		"--card": surface, "--card-foreground": fg,
		"--popover": surface, "--popover-foreground": fg,
		"--primary": primary, "--primary-foreground": contrastingText(primary),
		"--secondary": selection, "--secondary-foreground": fg,
		"--muted": surface, "--muted-foreground": color("light_foreground", fg),
		"--accent": selection, "--accent-foreground": fg,
		"--destructive": destructive, "--destructive-foreground": contrastingText(destructive),
		"--border": border, "--input": border, "--ring": primary,
		"--chart-1": primary, "--chart-2": color("green", primary),
		"--chart-3": color("yellow", primary), "--chart-4": color("magenta", primary), "--chart-5": color("cyan", primary),
		"--sidebar": color("dark_background", bg), "--sidebar-foreground": fg,
		"--sidebar-primary": primary, "--sidebar-primary-foreground": contrastingText(primary),
		"--sidebar-accent": selection, "--sidebar-accent-foreground": fg,
		"--sidebar-border": border, "--sidebar-ring": primary,
	}}
}

// Relative luminance and contrast follow the WCAG sRGB definition.
func relativeLuminance(hex string) float64 {
	value, _ := strconv.ParseUint(hex[1:], 16, 32)
	channel := func(v uint64) float64 {
		c := float64(v&255) / 255
		if c <= 0.04045 {
			return c / 12.92
		}
		return math.Pow((c+0.055)/1.055, 2.4)
	}
	return 0.2126*channel(value>>16) + 0.7152*channel(value>>8) + 0.0722*channel(value)
}

func contrastingText(background string) string {
	luminance := relativeLuminance(background)
	if (luminance+0.05)/0.05 >= 1.05/(luminance+0.05) {
		return "#000000"
	}
	return "#ffffff"
}
