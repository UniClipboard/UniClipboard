package uikit

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"testing"

	"github.com/egoist/mygo/ui"
)

func testImage(t *testing.T) ui.ImageSource {
	t.Helper()
	im := image.NewRGBA(image.Rect(0, 0, 40, 30))
	for x := 0; x < 40; x++ {
		for y := 0; y < 30; y++ {
			im.Set(x, y, color.RGBA{200, 80, 40, 255})
		}
	}
	var b bytes.Buffer
	if err := png.Encode(&b, im); err != nil {
		t.Fatal(err)
	}
	src, err := ui.DecodeImage(b.Bytes())
	if err != nil {
		t.Fatal(err)
	}
	return src
}

func TestQuickLookKeysAndClose(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		src := testImage(t)
		open := true
		var prev, next, primary int
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				ui.Text(c, "behind")
				r := k.QuickLook(c, QuickLookProps{
					Open: &open, Title: "2 of 21", CloseLabel: "Close", Src: src, Alt: "pic",
					PreviousLabel: "Previous", NextLabel: "Next", PrimaryLabel: "Copy",
				}, func() { ui.Text(c, "Open in Preview") })
				if r.Previous {
					prev++
				}
				if r.Next {
					next++
				}
				if r.Primary {
					primary++
				}
			})
		}
		tt := ui.NewTester(view, 900, 700)
		for _, s := range []string{"2 of 21", "Open in Preview", "Copy"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		tt.Key(0, ui.KeyRight)
		tt.Key(0, ui.KeyLeft)
		tt.Key(0, ui.KeyEnter)
		if prev != 1 || next != 1 || primary != 1 {
			t.Fatalf("keys: prev=%d next=%d primary=%d", prev, next, primary)
		}
		tt.Key(0, ui.KeySpace)
		if open {
			t.Fatal("Space did not close")
		}
		open = true
		tt.Frame()
		tt.Key(0, ui.KeyEscape)
		if open {
			t.Fatal("Escape did not close")
		}
	})
}
