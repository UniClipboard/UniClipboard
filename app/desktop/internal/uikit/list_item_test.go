package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func itemsABC() []ItemListItem {
	return []ItemListItem{
		{ID: "a", Title: "Alpha", Subtitle: "first"},
		{ID: "b", Title: "Bravo", Subtitle: "second", Highlight: []string{"rav"}},
		{ID: "c", Title: "Charlie", Monospace: true, Trailing: func() {}},
	}
}

func TestItemListSelectKeysDelete(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		var act, del []string
		var menu string
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.ItemList(c, &st, ItemListProps{Label: "Items", Items: itemsABC(),
				Menu: func(ids []string) []MenuEntry { return []MenuEntry{{ID: "x", Label: "Do"}} }}).Height(300)
			if id, ok := st.Activated(); ok {
				act = append(act, id)
			}
			if ids, ok := st.DeleteRequested(); ok {
				del = ids
			}
			if id, _, ok := st.MenuChosen(); ok {
				menu = id
			}
		}, 400, 400)
		if !tt.HasText("Alpha") || !tt.HasText("Charlie") {
			t.Fatalf("rows missing: %v", tt.Texts())
		}
		if err := tt.Click("Bravo"); err != nil {
			t.Fatal(err)
		}
		if got := st.Selection(); len(got) != 1 || got[0] != "b" {
			t.Fatalf("click select: %v", got)
		}
		tt.Key(0, ui.KeyDown)
		if got := st.Selection(); len(got) != 1 || got[0] != "c" {
			t.Fatalf("down: %v", got)
		}
		tt.Key(0, ui.KeyEnter)
		if len(act) != 1 || act[0] != "c" {
			t.Fatalf("enter: %v", act)
		}
		tt.Key(0, ui.KeyDelete)
		if len(del) != 1 || del[0] != "c" {
			t.Fatalf("delete: %v", del)
		}
		_ = menu
	})
}

func TestItemListMultiSelect(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.ItemList(c, &st, ItemListProps{Label: "Items", Items: itemsABC(), MultiSelect: true}).Height(300)
		}, 400, 400)
		_ = tt.Click("Alpha")
		tt.Key(ui.Shift, ui.KeyDown)
		if got := st.Selection(); len(got) != 2 {
			t.Fatalf("shift extend: %v", got)
		}
		if !st.Changed() && len(st.Selection()) == 0 {
			t.Fatal("no change")
		}
		st.Select("c")
		tt.Frame()
		if got := st.Selection(); len(got) != 1 || got[0] != "c" {
			t.Fatalf("Select: %v", got)
		}
	})
}

func TestItemListEmptyAndHighlight(t *testing.T) {
	runs, hit := highlightRuns("Bravo", []string{"rav"})
	if len(runs) != 3 || !hit[1] || runs[1] != "rav" {
		t.Fatalf("highlight: %v %v", runs, hit)
	}
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.ItemList(c, &st, ItemListProps{Label: "Empty"}).Height(100)
		}, 300, 200)
	})
}
