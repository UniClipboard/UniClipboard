package main

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"testing"
)

// Exercise the production macOS geometry on every platform, including Linux CI.
func TestTrayMacImageGeometry(t *testing.T) {
	cat, err := macCatTrayImage()
	if err != nil {
		t.Fatal(err)
	}
	catImage, err := png.Decode(bytes.NewReader(cat))
	if err != nil {
		t.Fatal(err)
	}
	for name, img := range map[string]image.Image{
		"glyph": renderIcon(macTrayIconSpec()),
		"cat":   catImage,
	} {
		t.Run(name, func(t *testing.T) {
			assertTrayMacBounds(t, name, img)
			partial, opaque := 0, 0
			for y := 0; y < macTrayImagePx; y++ {
				for x := 0; x < macTrayImagePx; x++ {
					c := color.NRGBAModel.Convert(img.At(x, y)).(color.NRGBA)
					if c.R != 0 || c.G != 0 || c.B != 0 {
						t.Fatalf("template pixel (%d,%d) is not black: %v", x, y, c)
					}
					if c.A > 0 && c.A < 255 {
						partial++
					}
					if c.A == 255 {
						opaque++
					}
				}
			}
			if partial == 0 || opaque == 0 {
				t.Fatalf("template needs anti-aliased edges and a solid shape: %d partial, %d opaque", partial, opaque)
			}
		})
	}
}

func assertTrayMacBounds(t *testing.T, style string, img image.Image) {
	t.Helper()
	if got := img.Bounds(); got != image.Rect(0, 0, 44, 44) {
		t.Fatalf("macOS image bounds = %v, want a 44 px square", got)
	}
	want := image.Rect(8, 7, 36, 37) // 28 x 30 px (14 x 15 pt), including anti-aliasing
	if style == "cat" {
		want = image.Rect(6, 6, 38, 37) // 32 x 31 px (16 x 15.5 pt); the source alpha is slightly asymmetric
	}
	// Check all alpha as well as the native e2e's visible-alpha threshold.
	for _, threshold := range []uint8{0, 20} {
		if bounds := trayAlphaBounds(img, threshold); bounds != want {
			t.Fatalf("macOS %s alpha > %d bounds = %v, want %v", style, threshold, bounds, want)
		}
	}
}

func trayAlphaBounds(img image.Image, threshold uint8) image.Rectangle {
	var bounds image.Rectangle
	for y := img.Bounds().Min.Y; y < img.Bounds().Max.Y; y++ {
		for x := img.Bounds().Min.X; x < img.Bounds().Max.X; x++ {
			_, _, _, a := img.At(x, y).RGBA()
			if a>>8 > uint32(threshold) {
				bounds = bounds.Union(image.Rect(x, y, x+1, y+1))
			}
		}
	}
	return bounds
}
