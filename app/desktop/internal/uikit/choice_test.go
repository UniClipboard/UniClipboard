package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestChoiceList(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		for _, disabled := range []bool{false, true} {
			got := ""
			view := func(c *ui.Context) {
				k.Apply(c)
				ui.Column(c).Padding(16).Children(func() {
					if r := k.ChoiceList(c, ChoiceListProps{Label: "Setup", Title: "Move or recover", Disabled: disabled, Items: []ChoiceListItem{
						{ID: "phone", Title: "Pair a phone", Description: "Scan a code", Icon: IconPhone},
						{ID: "restore", Title: "Restore from backup", Icon: IconPackage},
					}}); r.Selected != "" {
						got = r.Selected
					}
				})
			}
			tt := ui.NewTester(view, 500, 400)
			if !tt.HasText("Move or recover") || !tt.HasText("Scan a code") {
				t.Fatalf("texts: %v", tt.Texts())
			}
			_ = tt.Click("Restore from backup")
			if disabled && got != "" {
				t.Fatal("disabled list activated")
			}
			if !disabled {
				if got != "restore" {
					t.Fatalf("got=%q", got)
				}
				tt.Key(0, ui.KeyTab)
				tt.Key(0, ui.KeyEnter)
			}
		}
	})
}
