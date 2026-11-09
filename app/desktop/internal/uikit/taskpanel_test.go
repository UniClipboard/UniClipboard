package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTaskPanel(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		backs, nexts := 0, 0
		disabled := false
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Grow(1).Children(func() {
				back := k.TaskPanel(c, TaskPanelProps{
					Title: "Pair device", Description: "Enter the code", Stage: "Step 2 of 3",
					BackLabel: "Back", BackDisabled: disabled,
				}, func() { ui.Text(c, "form body") }, func() {
					if k.Button(c.Key("next"), "Next", ButtonProps{Variant: Primary}).Clicked() {
						nexts++
					}
				})
				if back {
					backs++
				}
			})
		}
		tt := ui.NewTester(view, 700, 600)
		for _, s := range []string{"Pair device", "Enter the code", "Step 2 of 3", "form body"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		_ = tt.Click("Back")
		_ = tt.Click("Next")
		if backs != 1 || nexts != 1 {
			t.Fatalf("backs=%d nexts=%d", backs, nexts)
		}
		disabled = true
		tt.Frame()
		_ = tt.Click("Back")
		if backs != 1 {
			t.Fatal("disabled Back fired")
		}
	})
}
