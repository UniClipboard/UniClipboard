package main

import (
	"bytes"
	_ "embed"
	"image"
	"image/png"

	xdraw "golang.org/x/image/draw"
)

// trayCatPNG is the tray image shipped in v1.1.2: a 44 px macOS template image, black pixels whose shape is in the alpha channel
// (icons/TRAY-CAT-SOURCE.md).
//
//go:embed icons/tray-cat@2x.png
var trayCatPNG []byte

// tintedCat draws the cat for a tray that shows its pixels as they are (Windows, Linux): the image's alpha, scaled to the platform's
// size and filled with its foreground colour. A black template image as is would vanish on a dark taskbar or panel.
func tintedCat(s iconSpec) ([]byte, error) {
	src, err := png.Decode(bytes.NewReader(trayCatPNG))
	if err != nil {
		return nil, err
	}
	mask := image.NewAlpha(image.Rect(0, 0, s.size, s.size))
	xdraw.CatmullRom.Scale(mask, mask.Bounds(), src, src.Bounds(), xdraw.Src, nil)
	img := image.NewNRGBA(mask.Bounds())
	for i, a := range mask.Pix {
		if a == 0 {
			continue
		}
		img.Pix[i*4], img.Pix[i*4+1], img.Pix[i*4+2] = s.pal.fg.R, s.pal.fg.G, s.pal.fg.B
		img.Pix[i*4+3] = uint8(uint16(a) * uint16(s.pal.fg.A) / 255)
	}
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
