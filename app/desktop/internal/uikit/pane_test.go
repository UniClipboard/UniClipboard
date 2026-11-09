package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestPane(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Row(c).Fill().Children(func() {
				for i, kind := range []PaneKind{PaneList, PaneDetail, PaneMain} {
					k.Pane(c.Key(i), PaneProps{Kind: kind, Label: "pane"}, func() { k.Text(c, []string{"A", "B", "C"}[i], TextProps{}) })
				}
			})
		}
		tt := ui.NewTester(view, 1200, 500)
		for _, s := range []string{"A", "B", "C"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %s", s)
			}
		}
	})
}
