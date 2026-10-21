package main

import (
	"bytes"
	"errors"
	"image/color"
	"image/png"
	"runtime"
	"testing"
)

// The tray icon executor against the failure modes F9-F13 of docs/architecture/gui-go-tray-icon.md. The native icon call is replaced by
// a recorder, so these checks prove which images are handed to the tray and how often, not what the tray displays.

type iconRecorder struct {
	failNext bool
	rendered []trayIconStyle
	applied  [][]byte
}

func (r *iconRecorder) icon() *trayIcon {
	return &trayIcon{
		render: func(style trayIconStyle) ([]byte, error) {
			r.rendered = append(r.rendered, style)
			if r.failNext {
				r.failNext = false
				return nil, errors.New("render failed")
			}
			return trayIconImage(style)
		},
		apply: func(data []byte) { r.applied = append(r.applied, data) },
	}
}

func TestTrayIconStyleCalls(t *testing.T) {
	r := &iconRecorder{}
	icon := r.icon()
	icon.show() // startup, before any settings read
	steps := []struct {
		name  string
		style string
		calls int
		shown trayIconStyle
	}{
		{"first read keeps the default", "glyph", 1, trayIconGlyph},
		{"cat", "cat", 2, trayIconCat},
		{"watcher read of the same value (F10)", "cat", 2, trayIconCat},
		{"page trigger of the same value (F10)", "cat", 2, trayIconCat},
		{"unknown value falls back to the default (F9)", "solid-cat", 3, trayIconGlyph},
		{"empty value is the default too (F9)", "", 3, trayIconGlyph},
	}
	for _, s := range steps {
		icon.setStyle(s.style)
		if len(r.applied) != s.calls || icon.shown != s.shown {
			t.Fatalf("%s: %d native calls, shown %q; want %d, %q", s.name, len(r.applied), icon.shown, s.calls, s.shown)
		}
	}
}

func TestTrayIconRenderFailureRetries(t *testing.T) {
	r := &iconRecorder{}
	icon := r.icon()
	icon.show()
	r.failNext = true
	icon.setStyle("cat") // F11: nothing is applied and the shown style stays
	if len(r.applied) != 1 || icon.shown != trayIconGlyph {
		t.Fatalf("after a failed render: %d native calls, shown %q", len(r.applied), icon.shown)
	}
	icon.setStyle("cat") // the next read tries again
	if len(r.applied) != 2 || icon.shown != trayIconCat {
		t.Fatalf("after the retry: %d native calls, shown %q", len(r.applied), icon.shown)
	}
}

func TestTrayIconThemeRepaintKeepsStyle(t *testing.T) {
	r := &iconRecorder{}
	icon := r.icon()
	icon.show()
	icon.setStyle("cat")
	icon.show() // F12: a theme change repaints the shown style
	if got := r.rendered[len(r.rendered)-1]; got != trayIconCat || len(r.applied) != 3 {
		t.Fatalf("theme repaint rendered %q with %d native calls", got, len(r.applied))
	}
	cat, _ := trayIconImage(trayIconCat)
	if !bytes.Equal(r.applied[2], cat) {
		t.Fatal("theme repaint did not hand the cat image to the tray")
	}
}

// The cat image per platform: the shipped template bytes on macOS, a tinted image of the platform tray size elsewhere.
func TestTrayCatImage(t *testing.T) {
	data, err := trayIconImage(trayIconCat)
	if err != nil {
		t.Fatal(err)
	}
	if runtime.GOOS == "darwin" {
		if !bytes.Equal(data, trayCatPNG) {
			t.Fatal("macOS does not receive the shipped template bytes")
		}
	}
	tinted, err := tintedCat(trayIconSpec())
	if err != nil {
		t.Fatal(err)
	}
	img, err := png.Decode(bytes.NewReader(tinted))
	if err != nil {
		t.Fatal(err)
	}
	spec := trayIconSpec()
	if b := img.Bounds(); b.Dx() != spec.size || b.Dy() != spec.size {
		t.Fatalf("tinted cat is %v, want %d px", b, spec.size)
	}
	opaque := 0
	for y := 0; y < spec.size; y++ {
		for x := 0; x < spec.size; x++ {
			c := color.NRGBAModel.Convert(img.At(x, y)).(color.NRGBA)
			if c.A == 0 {
				continue
			}
			opaque++
			if c.R != spec.pal.fg.R || c.G != spec.pal.fg.G || c.B != spec.pal.fg.B {
				t.Fatalf("pixel (%d,%d) is %v, want the platform foreground %v", x, y, c, spec.pal.fg)
			}
		}
	}
	if opaque == 0 {
		t.Fatal("tinted cat is empty")
	}
}
