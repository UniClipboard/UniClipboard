package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// fieldEachKit runs fn once per skin x colour scheme.
func fieldEachKit(t *testing.T, fn func(t *testing.T, k Kit)) {
	t.Helper()
	for _, s := range []SkinID{Linux, MacOS, Windows} {
		for _, d := range []bool{false, true} {
			k := Kit{Skin: s, Dark: d}
			t.Run(string(s)+"/"+k.scheme(), func(t *testing.T) { fn(t, k) })
		}
	}
}

func TestTextField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		text, submits, escapes := "", 0, 0
		off := "locked"
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Gap(10).Children(func() {
				r := k.TextField(c.Key("name"), TextFieldProps{Label: "Name", Value: &text, Placeholder: "Tag name", Error: "exists"})
				if r.Submitted {
					submits++
				}
				if r.Escaped {
					escapes++
				}
				k.TextField(c.Key("off"), TextFieldProps{Label: "Off", Value: &off, Disabled: true})
			})
		}
		tt := ui.NewTester(view, 400, 300)
		if err := tt.Click("Name"); err != nil {
			t.Fatal(err)
		}
		tt.Type("你好 abc")
		if text != "你好 abc" {
			t.Fatalf("text=%q", text)
		}
		tt.Key(0, ui.KeyEnter)
		if submits != 1 {
			t.Fatalf("submits=%d", submits)
		}
		tt.Key(0, ui.KeyEscape)
		if escapes != 1 {
			t.Fatalf("escapes=%d", escapes)
		}
		if !tt.HasText("exists") {
			t.Fatal("error text missing")
		}
		_ = tt.Click("Off")
		tt.Type("x")
		if off != "locked" {
			t.Fatalf("disabled field edited: %q", off)
		}
	})
}

func TestSearchField(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		q, submits := "", 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Children(func() {
				if k.SearchField(c, SearchFieldProps{Label: "Search", Value: &q, Placeholder: "Search", ShortcutHint: "Ctrl+F"}).Submitted {
					submits++
				}
			})
		}
		tt := ui.NewTester(view, 400, 200)
		if !tt.HasText("Ctrl+F") {
			t.Fatal("shortcut hint missing")
		}
		if err := tt.Click("Search"); err != nil {
			t.Fatal(err)
		}
		tt.Type("abc")
		tt.Key(0, ui.KeyEnter)
		if q != "abc" || submits != 1 {
			t.Fatalf("q=%q submits=%d", q, submits)
		}
		if tt.HasText("Ctrl+F") {
			t.Fatal("hint should hide while there is text")
		}
		if _, ok := tt.AccessByRole(ui.RoleSearch, ""); !ok {
			t.Log("search landmark not found by empty name")
		}
	})
}

func TestSearchToken(t *testing.T) {
	fieldEachKit(t, func(t *testing.T, k Kit) {
		removed := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(16).Gap(6).Children(func() {
				for i, kind := range []SearchTokenKind{SearchKindDefault, SearchKindTag, SearchKindSource, SearchKindTime, SearchKindType, SearchKindOther} {
					r := k.SearchToken(c.Key(i), SearchTokenProps{Field: "from:", Value: "iPhone", Kind: kind, RemoveLabel: "Remove token"})
					if r.Removed {
						removed++
					}
				}
			})
		}
		tt := ui.NewTester(view, 400, 300)
		if err := tt.Click("Remove token"); err != nil {
			t.Fatal(err)
		}
		if removed != 1 {
			t.Fatalf("removed=%d", removed)
		}
	})
}
