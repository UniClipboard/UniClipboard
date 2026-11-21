// Package boardui implements native mygo controls derived from the pinned BoardUI MIT source.
package boardui

import (
	"embed"
	"fmt"
	"strings"
	"sync"

	"github.com/egoist/mygo/ui"
)

//go:embed assets/Inter.ttf
var fontData []byte

//go:embed assets/*.txt assets/source-lock.json
var Notices embed.FS

// FontFamily is the registered family used by both native and reference fixtures.
const FontFamily = "BoardUI Inter"

var fontOnce sync.Once
var fontError error

// LoadFonts registers the pinned, embedded Inter font once per process.
func LoadFonts() error {
	fontOnce.Do(func() { fontError = ui.RegisterFont(fontData, FontFamily) })
	return fontError
}

// TypeStyle preserves one complete source typography style. Dimensions are DIPs.
type TypeStyle struct {
	Name                            string
	Size, LineHeight, LetterSpacing float32
	Weight                          int
}

// Metrics contains source-computed dimensions for one control size.
type Metrics struct{ Height, Radius, PaddingX, PaddingY, IconSize, Gap, BorderWidth float32 }

// Shadow is one ordered CSS shadow layer.
type Shadow struct {
	X, Y, Blur, Spread float32
	Color              ui.Color
	Inset              bool
}

// Token returns a source-resolved sRGB color. Unknown tokens are programming errors.
func Token(dark bool, name string) ui.Color {
	name = strings.TrimPrefix(name, "--color-")
	if c, ok := generatedColors[dark][name]; ok {
		return c
	}
	panic("boardui: unknown token " + name)
}

// Typography returns the full source style, never a size-only approximation.
func Typography(name string) TypeStyle {
	name = strings.TrimPrefix(name, "text-")
	if s, ok := generatedTypography[name]; ok {
		return s
	}
	panic("boardui: unknown typography " + name)
}

// Geometry returns the exact source-computed metrics for component and size.
func Geometry(component, size string) Metrics {
	if s, ok := generatedMetrics[component+"/"+size]; ok {
		return s
	}
	panic("boardui: unknown geometry " + component + "/" + size)
}

// Number returns a measured or parsed scalar from the locked source/fixture.
func Number(name string) float32 {
	if v, ok := generatedNumbers[name]; ok {
		return v
	}
	panic("boardui: unknown number " + name)
}

// Gradient returns the start/end colors of a two-stop source gradient.
func Gradient(dark bool, name string) (ui.Color, ui.Color) {
	if v, ok := generatedGradients[dark][name]; ok {
		return v[0], v[1]
	}
	panic("boardui: unknown gradient " + name)
}

// Shadows returns a copy of the ordered source shadow layers.
func Shadows(dark bool, name string) []Shadow {
	if v, ok := generatedShadows[dark][name]; ok {
		return append([]Shadow(nil), v...)
	}
	panic("boardui: unknown shadow " + name)
}

var iconOnce sync.Once
var parsedIcons map[string]*ui.SVG

// IconSVG resolves a finite, source-bound glyph set. There is no per-widget cache.
func IconSVG(name string) *ui.SVG {
	iconOnce.Do(func() {
		parsedIcons = make(map[string]*ui.SVG, len(generatedIcons))
		for name, source := range generatedIcons {
			svg, err := ui.ParseSVG([]byte(source))
			if err != nil {
				panic(fmt.Sprintf("boardui: icon %s: %v", name, err))
			}
			parsedIcons[name] = svg
		}
	})
	if svg, ok := parsedIcons[name]; ok {
		return svg
	}
	panic("boardui: unknown icon " + name)
}
