package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestPreviewFrame(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		for _, kind := range []PreviewKind{PreviewPlain, PreviewCode, PreviewImage, PreviewFile} {
			fit := 0
			tt := ui.NewTester(func(c *ui.Context) {
				k.Apply(c)
				k.PreviewFrame(c, PreviewFrameProps{
					Kind: kind, Caption: "3 lines", CaptionTrailing: "Space",
					TopEnd: func() {
						if k.Button(c.Key("fit"), "Fit", ButtonProps{}).Clicked() {
							fit++
						}
					},
					BottomStart: func() { k.Text(c, "QL", TextProps{}) },
				}, func() { k.Text(c, "Content", TextProps{}) })
			}, 600, 500)
			for _, s := range []string{"3 lines", "Space", "Content", "QL"} {
				if !tt.HasText(s) {
					t.Fatalf("kind %d missing %q", kind, s)
				}
			}
			if err := tt.Click("Fit"); err != nil || fit != 1 {
				t.Fatalf("kind %d corner control: %v %d", kind, err, fit)
			}
			_ = tt.Image()
		}
	})
}
