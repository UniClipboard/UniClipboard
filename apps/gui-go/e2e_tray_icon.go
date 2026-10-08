//go:build e2e

package main

import (
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// controlTrayIcon handles the tray icon checks of the e2e build:
//
//	tray-icon-compare:<dir>  the glyph's shape against the design board raster <dir>/glyph.png, and the image the tray really receives:
//	                         template alpha at sampled points, no clipping, and the visible size on macOS
//	tray-icon-export:<dir>   the image the running process hands to the tray and the glyph at the design geometry, as PNG files
func (s *EvidenceService) controlTrayIcon(action string) (bool, error) {
	switch {
	case strings.HasPrefix(action, "tray-icon-compare:"):
		detail, ok := compareTrayIconToDesign(strings.TrimPrefix(action, "tray-icon-compare:"))
		return true, s.write(Step{Window: "tray", Step: "tray-icon-compare", OK: ok, Detail: detail})
	case strings.HasPrefix(action, "tray-icon-export:"):
		dir := strings.TrimPrefix(action, "tray-icon-export:")
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: false, Detail: err.Error()})
		}
		files := map[string]*image.NRGBA{
			"platform.png":  renderIcon(trayIconSpec()), // what the running process hands to the tray
			"reference.png": renderIcon(iconSpec{size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{0x1D, 0x1D, 0x1F, 255}}}),
		}
		for name, img := range files {
			f, err := os.Create(filepath.Join(dir, name))
			if err == nil {
				err = png.Encode(f, img)
				_ = f.Close()
			}
			if err != nil {
				return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: false, Detail: err.Error()})
			}
		}
		return true, s.write(Step{Window: "tray", Step: "tray-icon-export", OK: true, Detail: map[string]any{"dir": dir, "files": len(files)}})
	}
	return false, nil
}

// glyphReference is the design board rasterized independently (e2e/tray-icon-design). The samples are in design units on the production
// frame: a point that must be transparent (the inside of the front card) and one that must be opaque (the centre line of its top edge).
const glyphReference = "glyph.png"

var (
	glyphClearAt = [2]float64{13.5, 10.5}
	glyphSolidAt = [2]float64{13.5, 3}
)

// compareTrayIconToDesign checks the glyph twice. First its shape: rendered with the design's macOS-light colours on the design's 44 px / 36 px
// canvas, it is compared with the independently rasterized design board by counting pixels that differ by more than fuzz (out of 255) in any
// channel. That is a shape check only: the design's own geometry is not what the tray receives. Second the PRODUCTION frame, rendered with
// trayIconSpec, which is the image handed to the tray: the template alpha rule at the sampled points, no clipping at the image edge, and on
// macOS a long side of 17 to 19 pt (the design's own 18 pt grid would show a smaller glyph next to neighbouring status items).
func compareTrayIconToDesign(dir string) (map[string]any, bool) {
	const fuzz = 77 // 30%: anti-aliasing differences between two rasterizers
	bg := color.NRGBA{0xE9, 0xE9, 0xEC, 255}
	f, err := os.Open(filepath.Join(dir, glyphReference))
	if err != nil {
		return map[string]any{"error": err.Error()}, false
	}
	ref, err := png.Decode(f)
	_ = f.Close()
	if err != nil {
		return map[string]any{"error": err.Error()}, false
	}
	mine := renderIcon(iconSpec{size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{0x1D, 0x1D, 0x1F, 255}}})
	differing, worst := 0, 0
	for y := 0; y < 44; y++ {
		for x := 0; x < 44; x++ {
			c := mine.NRGBAAt(x, y)
			a := int(c.A)
			over := [3]int{(int(c.R)*a + int(bg.R)*(255-a)) / 255, (int(c.G)*a + int(bg.G)*(255-a)) / 255, (int(c.B)*a + int(bg.B)*(255-a)) / 255}
			rr, rg, rb, _ := ref.At(x, y).RGBA()
			d := 0
			for i, rv := range [3]int{int(rr >> 8), int(rg >> 8), int(rb >> 8)} {
				if diff := abs(over[i] - rv); diff > d {
					d = diff
				}
			}
			if d > fuzz {
				differing++
			}
			if d > worst {
				worst = d
			}
		}
	}
	// The tolerance is a small fraction of the 44 x 44 image: two rasterizers disagree only along edges.
	shapeOK := differing <= 30
	results := map[string]any{"shape": map[string]any{"differingPixels": differing, "worstChannelDiff": worst, "pass": shapeOK}}

	spec := trayIconSpec()
	prod := renderIcon(iconSpec{size: spec.size, art: spec.art, pal: iconPalette{fg: color.NRGBA{A: 255}}})
	at := func(u [2]float64) uint8 {
		off := (float64(spec.size) - spec.art) / 2
		return prod.NRGBAAt(int(off+u[0]*spec.art/24), int(off+u[1]*spec.art/24)).A
	}
	clearA, solidA := at(glyphClearAt), at(glyphSolidAt)
	alphaOK := clearA == 0 && solidA == 255
	results["templateAlphaProduction"] = map[string]any{"imagePx": spec.size, "artPx": spec.art, "clearAlpha": clearA, "solidAlpha": solidA, "pass": alphaOK}

	x0, y0, x1, y1 := spec.size, spec.size, -1, -1
	for y := 0; y < spec.size; y++ {
		for x := 0; x < spec.size; x++ {
			if prod.NRGBAAt(x, y).A > 20 {
				x0, y0, x1, y1 = min(x0, x), min(y0, y), max(x1, x), max(y1, y)
			}
		}
	}
	widthPx, heightPx := x1-x0+1, y1-y0+1
	clipped := x1 < 0 || x0 == 0 || y0 == 0 || x1 == spec.size-1 || y1 == spec.size-1
	sizeOK := !clipped
	if runtime.GOOS == "darwin" {
		longest := max(widthPx, heightPx)
		sizeOK = sizeOK && longest >= 34 && longest <= 38 // 17 to 19 pt at 2x
	}
	results["productionSize"] = map[string]any{"widthPx": widthPx, "heightPx": heightPx, "clipped": clipped, "pass": sizeOK}
	return results, shapeOK && alphaOK && sizeOK
}

func abs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
