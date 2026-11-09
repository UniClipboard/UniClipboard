package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSegmentedControl(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		v := "list"
		changes := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Children(func() {
				r := k.SegmentedControl(c, SegmentedControlProps{Label: "View", Value: &v, Items: []SegmentedItem{
					{Value: "list", Label: "List", Icon: IconList},
					{Value: "grid", Label: "Grid", Icon: IconGrid, IconOnly: true},
					{Value: "text", Label: "Text"},
				}})
				if r.Changed {
					changes++
				}
			})
		}
		tt := ui.NewTester(view, 500, 200)
		if err := tt.Click("Text"); err != nil {
			t.Fatal(err)
		}
		if v != "text" || changes != 1 {
			t.Fatalf("v=%q changes=%d", v, changes)
		}
		tt.Key(0, ui.KeyLeft)
		if v != "grid" {
			t.Fatalf("arrow key should move selection: %q", v)
		}
		if info, ok := tt.AccessByRole(ui.RoleRadio, "Grid"); !ok || !info.Checked {
			t.Fatalf("grid radio: %+v ok=%v", info, ok)
		}
	})
}
