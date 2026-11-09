package uikit

import (
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
)

func gridItems(n int) []GridItem {
	out := make([]GridItem, n)
	for i := range out {
		kind := []ContentKind{KindText, KindCode, KindLink, KindImage}[i%4]
		out[i] = GridItem{ID: "g" + strconv.Itoa(i), Title: "Card " + strconv.Itoa(i), Subtitle: "sub", Kind: kind, Text: "body"}
	}
	return out
}

func TestGroupedGrid(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		var del []string
		act := ""
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.GroupedGrid(c, &st, GroupedGridProps{Label: "Grid", Items: gridItems(12),
				Groups: []ListGroup{{ID: "a", Title: "Group A", Count: 12}}})
			if ids, ok := st.DeleteRequested(); ok {
				del = ids
			}
			if id, ok := st.Activated(); ok {
				act = id
			}
		}, 700, 500)
		tt.Frame()
		if !tt.HasText("Group A") || !tt.HasText("Card 0") {
			t.Fatalf("missing: %v", tt.Texts())
		}
		if err := tt.Click("Card 1"); err != nil {
			t.Fatal(err)
		}
		if got := st.Selection(); len(got) != 1 || got[0] != "g1" {
			t.Fatalf("click: %v", got)
		}
		tt.Key(0, ui.KeyRight)
		if got := st.Selection(); len(got) != 1 || got[0] != "g2" {
			t.Fatalf("right: %v", got)
		}
		tt.Key(ui.Shift, ui.KeyRight)
		if got := st.Selection(); len(got) != 2 {
			t.Fatalf("shift: %v", got)
		}
		tt.Key(0, ui.KeyDown)
		if got := st.Selection(); len(got) != 1 || got[0] == "g3" {
			t.Fatalf("down should move a row: %v", got)
		}
		tt.Key(ui.Cmd, ui.KeyA)
		if len(st.Selection()) != 12 {
			t.Fatalf("select all: %v", st.Selection())
		}
		tt.Key(0, ui.KeyDelete)
		if len(del) != 12 {
			t.Fatalf("delete: %v", del)
		}
		tt.Key(0, ui.KeyEnter)
		if act == "" {
			t.Fatal("enter did not activate")
		}
	})
}

func TestGroupedGridReveal(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		vp := &HistoryWindowViewport{Total: 100}
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.GroupedGrid(c, &st, GroupedGridProps{
				Label: "Grid", Items: gridItems(100), Viewport: vp,
				Groups: []ListGroup{{ID: "today", Title: "Today", Count: 100}},
			})
		}, 700, 400)
		tt.Frame()
		if tt.HasText("Card 80") {
			t.Fatal("off-screen card was built before reveal")
		}
		vp.Reveal = "g80"
		tt.Frame()
		tt.Frame()
		if !tt.HasText("Card 80") {
			t.Fatalf("reveal did not reach the visual grid row: %v", tt.Texts())
		}
	})
}
