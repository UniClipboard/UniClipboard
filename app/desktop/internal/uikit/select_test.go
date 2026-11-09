package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSelect(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		for _, filter := range []bool{false, true} {
			v, off := "", "b"
			items := []SelectItem{{"a", "Alpha"}, {"b", "Beta"}, {"c", "Gamma"}}
			view := func(c *ui.Context) {
				k.Apply(c)
				ui.Column(c).Padding(16).Gap(8).Children(func() {
					k.Select(c.Key("s"), SelectProps{Items: items, Value: &v, Label: "Kind", Placeholder: "Pick one", Filter: filter})
					k.Select(c.Key("d"), SelectProps{Items: items, Value: &off, Label: "Locked", Disabled: true})
				})
			}
			tt := ui.NewTester(view, 400, 400)
			if !tt.HasText("Pick one") {
				t.Fatalf("placeholder missing: %v", tt.Texts())
			}
			if err := tt.Click("Kind"); err != nil {
				t.Fatal(err)
			}
			if err := tt.Click("Gamma"); err != nil {
				t.Fatal(err)
			}
			if v != "c" {
				t.Fatalf("filter=%v v=%q", filter, v)
			}
			if !tt.HasText("Gamma") || tt.HasText("Pick one") {
				t.Fatalf("chosen label not shown: %v", tt.Texts())
			}
			tt.Key(0, ui.KeyDown) // trigger keeps focus; arrows open/move
			tt.Key(0, ui.KeyEnter)
			_ = tt.Click("Locked")
			if off != "b" {
				t.Fatal("disabled select changed")
			}
			if info, ok := tt.AccessByRole(ui.RoleComboBox, "Locked"); ok && !info.Disabled {
				t.Fatalf("disabled select not exposed as disabled: %+v", info)
			}
		}
	})
}
