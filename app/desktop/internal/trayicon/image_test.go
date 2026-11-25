package trayicon

import (
	"bytes"
	"image/png"
	"runtime"
	"testing"
)

func TestRenderStyles(t *testing.T) {
	t.Run("glyph", func(t *testing.T) {
		raw, template, err := Render("glyph")
		if err != nil {
			t.Fatalf("Render(glyph): %v", err)
		}
		if want := runtime.GOOS != "linux"; template != want {
			t.Fatalf("glyph template = %v, want %v", template, want)
		}
		img, err := png.Decode(bytes.NewReader(raw))
		if err != nil {
			t.Fatalf("decode glyph PNG: %v", err)
		}
		if img.Bounds().Dx() < 16 || img.Bounds().Dx() != img.Bounds().Dy() {
			t.Fatalf("unexpected glyph dimensions: %v", img.Bounds())
		}
	})

	t.Run("cat", func(t *testing.T) {
		raw, template, err := Render("cat")
		if err != nil {
			t.Fatalf("Render(cat): %v", err)
		}
		if template {
			t.Fatal("cat style must render as full-color non-template artwork")
		}
		img, err := png.Decode(bytes.NewReader(raw))
		if err != nil {
			t.Fatalf("decode cat PNG: %v", err)
		}
		b := img.Bounds()
		if b.Dx() < 16 || b.Dx() != b.Dy() {
			t.Fatalf("unexpected cat dimensions: %v", b)
		}
		// Verify both bright white face pixels and dark outline/eye pixels survive scaling.
		var hasBright, hasDark, hasTransparent bool
		for y := b.Min.Y; y < b.Max.Y; y++ {
			for x := b.Min.X; x < b.Max.X; x++ {
				r, g, bl, a := img.At(x, y).RGBA()
				if a == 0 {
					hasTransparent = true
					continue
				}
				if a > 0xc000 {
					if r > 0xd000 && g > 0xd000 && bl > 0xd000 {
						hasBright = true
					}
					if r < 0x4000 && g < 0x4000 && bl < 0x4000 {
						hasDark = true
					}
				}
			}
		}
		if !hasTransparent || !hasBright || !hasDark {
			t.Fatalf("cat artwork lost expected contrast channels: transparent=%v bright=%v dark=%v", hasTransparent, hasBright, hasDark)
		}
	})

	t.Run("invalid", func(t *testing.T) {
		if _, _, err := Render("unknown"); err == nil {
			t.Fatal("expected error for invalid style")
		}
	})
}
