package uikit

import (
	"strconv"
	"testing"

	"github.com/egoist/mygo/ui"
)

func manyItems(n int) []ItemListItem {
	out := make([]ItemListItem, n)
	for i := range out {
		out[i] = ItemListItem{ID: "i" + strconv.Itoa(i), Title: "Entry " + strconv.Itoa(i), Subtitle: "sub"}
	}
	return out
}

func TestGroupedListGroupsKeysAndWindow(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		var del []string
		footer := 0
		items := manyItems(200)
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.GroupedList(c, &st, GroupedListProps{
				Label:  "History",
				Groups: []ListGroup{{ID: "g1", Title: "Today", Detail: "now", Trailing: "100 items", Count: 100}, {ID: "g2", Title: "Yesterday", Count: 100}},
				Items:  items,
				Footer: func() { footer++; ui.Text(c, "END") },
			})
			if ids, ok := st.DeleteRequested(); ok {
				del = ids
			}
		}, 500, 400)
		if !tt.HasText("Today") || !tt.HasText("Entry 0") {
			t.Fatalf("missing: %v", tt.Texts())
		}
		if tt.HasText("Entry 150") {
			t.Fatal("virtual list built an off-screen row")
		}
		if err := tt.Click("Entry 1"); err != nil {
			t.Fatal(err)
		}
		tt.Key(0, ui.KeyDown)
		tt.Key(ui.Shift, ui.KeyDown)
		if got := st.Selection(); len(got) != 2 || got[0] != "i2" {
			t.Fatalf("selection: %v", got)
		}
		tt.Key(ui.Cmd, ui.KeyA)
		if got := st.Selection(); len(got) != 200 {
			t.Fatalf("select all: %d", len(got))
		}
		tt.Key(0, ui.KeyDelete)
		if len(del) != 200 {
			t.Fatalf("delete: %d", len(del))
		}
		if footer == 0 {
			t.Fatal("footer not drawn")
		}
	})
}

func TestGroupedListViewportPlaceholders(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		items := manyItems(20)
		vp := &HistoryWindowViewport{Total: 100, Offset: 40}
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.GroupedList(c, &st, GroupedListProps{Label: "H", Viewport: vp, Items: items})
		}, 500, 400)
		tt.Frame()
		f, l := st.VisibleRange()
		if f > l {
			t.Fatalf("range %d..%d", f, l)
		}
		vp.Reveal = "i5"
		tt.Frame()
		tt.Frame()
		if !tt.HasText("Entry 5") {
			t.Fatalf("reveal did not scroll: %v", tt.Texts())
		}
	})
}
