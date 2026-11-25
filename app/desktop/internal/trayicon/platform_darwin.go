package trayicon

import (
	"context"
	"image/color"
)

func platformSpec() spec {
	return spec{size: 44, glyph: 36, cat: 38, foreground: color.NRGBA{A: 255}, template: true}
}
func watchTheme(context.Context, func()) {}
