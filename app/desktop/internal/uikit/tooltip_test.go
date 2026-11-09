package uikit

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestWithTooltip(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(60).Children(func() {
				b := ui.ButtonBase(c).Padding(8).Label("Pin")
				b.Children(func() { ui.Text(c, "Pin") })
				k.WithTooltip(c, b, "Pin this item")
				k.WithTooltip(c, ui.Text(c, "plain"), "")
			})
		}
		tt := ui.NewTester(view, 400, 300)
		if tt.HasText("Pin this item") {
			t.Fatal("tooltip shown without hover")
		}
		r, ok := tt.Find("Pin")
		if !ok {
			t.Fatal("no anchor")
		}
		tt.Move(r.X+r.W/2, r.Y+r.H/2)
		tt.Advance(2 * time.Second)
		if !tt.HasText("Pin this item") {
			t.Fatalf("tooltip not shown on hover: %v", tt.Texts())
		}
	})
}
