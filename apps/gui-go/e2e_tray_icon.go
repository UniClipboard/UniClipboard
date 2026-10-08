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
//	tray-icon-compare:<dir>  the design's resting cat rendered in the design's macOS-light colours against <dir>/synced.png, and the template
//	                         alpha rules (the eyes are transparent knock-outs, the face is opaque)
//	tray-icon-export:<dir>   the platform's own image (what the tray receives) and the design-colour image, as PNG files
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
			"platform.png":     renderIcon(trayIconSpec()),
			"reference-fg.png": renderIcon(iconSpec{size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{0x1D, 0x1D, 0x1F, 255}}}),
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

// compareTrayIconToDesign renders the resting cat with the design's macOS-light colours on its menu-bar background and counts the pixels
// that differ from the independently rasterized design board by more than fuzz (out of 255) in any channel.
func compareTrayIconToDesign(dir string) (map[string]any, bool) {
	const fuzz = 77 // 30%: anti-aliasing differences between two rasterizers
	bg := color.NRGBA{0xE9, 0xE9, 0xEC, 255}
	results := map[string]any{}
	f, err := os.Open(filepath.Join(dir, "synced.png"))
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
	pass := differing <= 30
	results["synced"] = map[string]any{"differingPixels": differing, "worstChannelDiff": worst, "pass": pass}
	// Template rule: macOS reads alpha only, so the eyes must be transparent and the face opaque in the production frame.
	template := renderIcon(iconSpec{size: 44, art: 36, pal: iconPalette{fg: color.NRGBA{A: 255}}})
	alphaAt := func(ux, uy float64) uint8 { return template.NRGBAAt(int(4+ux*36.0/24), int(4+uy*36.0/24)).A }
	samples := map[string]any{"eyeAlpha": alphaAt(9, 13.8), "faceAlpha": alphaAt(12, 17.5)}
	templateOK := samples["eyeAlpha"] == uint8(0) && samples["faceAlpha"] == uint8(255)
	results["templateAlpha"] = map[string]any{"samples": samples, "pass": templateOK}
	// Production size: the image the tray really receives must not clip the cat, and on macOS the cat must fill 17 to 19 pt of width (a size
	// that matches neighbouring status items; the design's own 18 pt grid showed only 14 pt).
	prod := renderIcon(trayIconSpec())
	size := prod.Bounds().Dx()
	x0, y0, x1, y1 := size, size, -1, -1
	for y := 0; y < size; y++ {
		for x := 0; x < size; x++ {
			if prod.NRGBAAt(x, y).A > 20 {
				x0, y0, x1, y1 = min(x0, x), min(y0, y), max(x1, x), max(y1, y)
			}
		}
	}
	widthPx, heightPx := x1-x0+1, y1-y0+1
	clipped := x0 == 0 || y0 == 0 || x1 == size-1 || y1 == size-1
	sizeOK := !clipped && x1 >= 0
	if runtime.GOOS == "darwin" {
		sizeOK = sizeOK && widthPx >= 34 && widthPx <= 38 // 17 to 19 pt at 2x
	}
	results["productionSize"] = map[string]any{"imagePx": size, "catWidthPx": widthPx, "catHeightPx": heightPx, "clipped": clipped, "pass": sizeOK}
	return results, pass && templateOK && sizeOK
}

func abs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
