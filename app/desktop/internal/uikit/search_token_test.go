package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTokenSearchField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		q, hi := "", 0
		tokens := []SearchFieldToken{{ID: "t1", Field: "Tag", Value: "#deploy", Kind: SearchKindTag, RemoveLabel: "Remove Tag #deploy"}}
		sugg := []SearchSuggestion{
			{ID: "a", Label: "arch-desktop", Group: "Sources", Kind: SearchKindSource, Detail: "12 items"},
			{ID: "b", Label: "iPhone", Kind: SearchKindSource, Selected: true},
			{ID: "c", Label: "Create #re", Create: true, Muted: true},
		}
		var res TokenSearchResult
		var keys []ui.Key
		picked, removed, cleared := -1, "", 0
		open := false
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Gap(8).Children(func() {
				res = k.TokenSearchField(c, TokenSearchFieldProps{
					Label: "Search clips", Placeholder: "Search", ShortcutHint: "Ctrl+F", Value: &q,
					Tokens: tokens, Suggestions: sugg, Highlighted: &hi, Open: &open,
					ClearLabel: "Clear search", SuggestionsFooter: "Up/Down to move", Feedback: "Literal search",
					OnKey: func(key SearchFieldKey) bool {
						keys = append(keys, key.Key)
						return key.Key == ui.KeyDown
					},
				})
				if res.Selected >= 0 {
					picked = res.Selected
				}
				if res.RemovedToken != "" {
					removed = res.RemovedToken
				}
				if res.Cleared {
					cleared++
				}
			})
		}
		tt := ui.NewTester(view, 600, 500)
		if tt.HasText("arch-desktop") {
			t.Fatal("menu shown without focus")
		}
		if err := tt.Click("Search clips"); err != nil {
			t.Fatal(err)
		}
		tt.Type("ab")
		if q != "ab" {
			t.Fatalf("q=%q", q)
		}
		for _, want := range []string{"arch-desktop", "Up/Down to move", "Literal search"} {
			if !tt.HasText(want) {
				t.Fatalf("menu lacks %q: %v", want, tt.Texts())
			}
		}
		if info, ok := tt.AccessByRole(ui.RoleComboBox, "Search clips"); !ok {
			t.Fatalf("combobox role missing: %+v", info)
		}
		tt.Key(0, ui.KeyDown)
		if len(keys) == 0 || keys[len(keys)-1] != ui.KeyDown {
			t.Fatalf("OnKey did not see Down: %v", keys)
		}
		if err := tt.Click("iPhone"); err != nil {
			t.Fatal(err)
		}
		if picked != 1 {
			t.Fatalf("picked=%d", picked)
		}
		if !tt.Focused("Search clips") {
			t.Fatal("clicking a suggestion must keep focus in the field")
		}
		if err := tt.Click("Remove Tag #deploy"); err != nil {
			t.Fatal(err)
		}
		if removed != "t1" {
			t.Fatalf("removed=%q", removed)
		}
		if err := tt.Click("Clear search"); err != nil {
			t.Fatal(err)
		}
		if cleared != 1 {
			t.Fatalf("cleared=%d", cleared)
		}
	})
}
