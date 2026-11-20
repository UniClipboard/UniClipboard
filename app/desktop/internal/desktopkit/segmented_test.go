package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionSegmentedControl(t *testing.T) {
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
		tt := compositionTester(t, view, 500, 200)
		if err := tt.Click("Text"); err != nil {
			t.Fatal(err)
		}
		if v != "text" || changes != 1 {
			t.Fatalf("v=%q changes=%d", v, changes)
		}
		tt.Key(0, ui.KeyLeft)
		if v != "text" {
			t.Fatalf("source arrows move focus without committing selection: %q", v)
		}
		if info, ok := tt.AccessByRole(ui.RoleToggleButton, "Grid"); !ok || !info.Focused {
			t.Fatalf("Grid focus after arrow: %+v ok=%v", info, ok)
		}
		tt.Key(0, ui.KeySpace)
		if v != "grid" || changes != 2 {
			t.Fatalf("Space commits grid: value=%q changes=%d", v, changes)
		}
		if info, ok := tt.AccessByRole(ui.RoleToggleButton, "Grid"); !ok || !info.Checked {
			t.Fatalf("grid toggle: %+v ok=%v", info, ok)
		}
	})
}
