package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestActionBar(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		n := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				k.ActionBar(c, ActionBarProps{Hint: "Return to copy"}, func() {
					if k.Button(c.Key("copy"), "Copy", ButtonProps{Variant: Primary}).Clicked() {
						n++
					}
				})
			})
		}
		tt := ui.NewTester(view, 600, 300)
		if !tt.HasText("Return to copy") {
			t.Fatal("hint missing")
		}
		if err := tt.Click("Copy"); err != nil || n != 1 {
			t.Fatalf("click: %v %d", err, n)
		}
	})
}
