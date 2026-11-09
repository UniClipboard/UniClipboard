package uikit

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func TestComboField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		text, hi := "re", -1
		color := TagColor("orange")
		sugg := []SearchSuggestion{
			{ID: "1", Label: "recipes", Swatch: "green", Detail: "3"},
			{ID: "2", Label: "Create #re", Create: true},
		}
		picked, dismissed := -1, 0
		var keys []ui.Key
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Gap(8).Children(func() {
				r := k.ComboField(c.Key("combo"), ComboFieldProps{
					Label: "Tag", Prefix: "#", Value: &text, Suggestions: sugg, Highlighted: &hi,
					Footer: "Return to add", MaxLength: 8,
					CreateColor: &ComboFieldCreateColor{Label: "Color", Colors: []TagColor{"orange", "blue", "#ff00aa"}, Value: &color},
					OnKey:       func(key SearchFieldKey) bool { keys = append(keys, key.Key); return false },
				})
				if r.Selected >= 0 {
					picked = r.Selected
				}
				if r.Dismissed {
					dismissed++
				}
			})
		}
		tt := ui.NewTester(view, 600, 500)
		if err := tt.Click("Tag"); err != nil {
			t.Fatal(err)
		}
		if !tt.HasText("recipes") || !tt.HasText("Return to add") {
			t.Fatalf("menu missing: %v", tt.Texts())
		}
		tt.Type("x")
		if text != "rex" {
			t.Fatalf("text=%q", text)
		}
		tt.Key(0, ui.KeyDown)
		if len(keys) == 0 {
			t.Fatal("OnKey saw no key")
		}
		if err := tt.Click("blue"); err != nil {
			t.Fatal(err)
		}
		if color != "blue" {
			t.Fatalf("color=%q", color)
		}
		if err := tt.Click("recipes"); err != nil {
			t.Fatal(err)
		}
		if picked != 0 {
			t.Fatalf("picked=%d", picked)
		}
		tt.ClickAt(590, 490) // empty window area: focus leaves the field and its menu
		tt.Advance(time.Second)
		if dismissed != 1 {
			t.Fatalf("dismissed=%d", dismissed)
		}
	})
}
