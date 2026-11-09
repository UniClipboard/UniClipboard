package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTable(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		var del []string
		cell := func(s string) func(c *ui.Context) {
			return func(c *ui.Context) { ui.Text(c, s) }
		}
		rows := []TableRow{
			{ID: "r1", Cells: map[string]func(*ui.Context){"name": cell("Phone"), "state": cell("online")}},
			{ID: "r2", Cells: map[string]func(*ui.Context){"name": cell("Laptop"), "state": cell("offline")}},
			{ID: "r3", Cells: map[string]func(*ui.Context){"name": cell("Tablet")}},
		}
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.Table(c, &st, TableProps{Label: "Devices",
				Columns: []TableColumn{{ID: "name", Label: "Name", Fill: true}, {ID: "state", Label: "State", Width: 100, Align: ui.End}},
				Rows:    rows}).Height(300)
			if ids, ok := st.DeleteRequested(); ok {
				del = ids
			}
		}, 500, 400)
		if !tt.HasText("Phone") || !tt.HasText("State") {
			t.Fatalf("missing: %v", tt.Texts())
		}
		if err := tt.Click("Laptop"); err != nil {
			t.Fatal(err)
		}
		if got := st.Selection(); len(got) != 1 || got[0] != "r2" {
			t.Fatalf("click: %v", got)
		}
		tt.Key(ui.Shift, ui.KeyDown)
		if len(st.Selection()) != 2 {
			t.Fatalf("shift: %v", st.Selection())
		}
		tt.Key(0, ui.KeyDelete)
		if len(del) != 2 {
			t.Fatalf("delete: %v", del)
		}
	})
}

func TestTableEmpty(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var st ListState
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.Table(c, &st, TableProps{Label: "T", Empty: "No devices"}).Height(200)
		}, 300, 300)
		if !tt.HasText("No devices") {
			t.Fatalf("empty text: %v", tt.Texts())
		}
	})
}
