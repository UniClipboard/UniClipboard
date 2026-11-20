package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionCodeField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		code := ""
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Children(func() {
				k.CodeField(c, CodeFieldProps{Label: "Pairing code", Length: 6, Value: &code})
			})
		}
		tt := compositionTester(t, view, 500, 300)
		tt.Key(0, ui.KeyTab)
		tt.Type("4a8-2")
		if code != "482" {
			t.Fatalf("only digits are kept: %q", code)
		}
		tt.Type("1399999")
		if code != "482139" {
			t.Fatalf("length is capped: %q", code)
		}
		if !tt.HasText("4") || !tt.HasText("9") {
			t.Fatalf("digits not drawn in boxes: %v", tt.Texts())
		}
		code = "１２３ ４５６"
		tt.Frame()
		if code != "123456" || !tt.HasText("6") {
			t.Fatalf("full-width input not normalized before rendering: %q", code)
		}
	})
}
