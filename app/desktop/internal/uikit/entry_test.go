package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestEntryPage(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		back, next := 0, 0
		view := func(c *ui.Context) {
			k.Apply(c)
			r := k.EntryPage(c, EntryPageProps{
				Status: EntryPageStatus{Text: "Setting up", Tone: EntryPositive, Busy: true},
				Title:  "Welcome", Description: "Pair your first device", BackLabel: "Go back",
			}, func() { k.Text(c, "StepBody", TextProps{}) }, func() {
				if k.Button(c, "Continue", ButtonProps{Variant: Primary, Size: SizeLarge}).Clicked() {
					next++
				}
			})
			if r.BackClicked {
				back++
			}
		}
		tt := ui.NewTester(view, 800, 600)
		for _, s := range []string{"Setting up", "Welcome", "Pair your first device", "StepBody"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Go back"); err != nil || back != 1 {
			t.Fatalf("back: %v %d", err, back)
		}
		if err := tt.Click("Continue"); err != nil || next != 1 {
			t.Fatalf("next: %v %d", err, next)
		}
	})
}
