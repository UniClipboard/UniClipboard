package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSheetDoneEscapeFocus(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		for _, variant := range []SheetVariant{SheetManagement, SheetTask} {
			open := false
			var done bool
			outside := 0
			view := func(c *ui.Context) {
				k.Apply(c)
				ui.Column(c).Padding(20).Gap(8).Children(func() {
					if ui.ButtonBase(c.Key("opener")).Label("Open").Padding(8).Children(func() { ui.Text(c, "Open") }).Clicked() {
						open = true
					}
					if ui.ButtonBase(c.Key("other")).Label("Other").Padding(8).Children(func() { ui.Text(c, "Other") }).Clicked() {
						outside++
					}
					if k.Sheet(c, SheetProps{
						Variant: variant, Open: &open, Title: "Devices", Done: "Done",
						Header: func() { ui.Text(c, "filter") },
						Footer: func() { ui.Text(c, "3 devices") },
					}, func() { ui.Text(c, "body row") }) {
						done = true
					}
				})
			}
			tt := ui.NewTester(view, 900, 700)
			if tt.HasText("body row") {
				t.Fatal("sheet visible while closed")
			}
			if err := tt.Click("Open"); err != nil {
				t.Fatal(err)
			}
			for _, s := range []string{"Devices", "filter", "body row", "3 devices", "Done"} {
				if !tt.HasText(s) {
					t.Fatalf("missing %q: %v", s, tt.Texts())
				}
			}
			// Tab must stay inside the sheet.
			for i := 0; i < 6; i++ {
				tt.Key(0, ui.KeyTab)
				if tt.Focused("Other") || tt.Focused("Open") {
					t.Fatal("focus escaped the sheet")
				}
			}
			if err := tt.Click("Done"); err != nil || !done {
				t.Fatalf("done: %v %v", err, done)
			}
			tt.Key(0, ui.KeyEscape)
			if open || tt.HasText("body row") {
				t.Fatal("Escape did not close the sheet")
			}
			if !tt.Focused("Open") {
				t.Fatal("focus did not return to the opener")
			}
		}
	})
}
