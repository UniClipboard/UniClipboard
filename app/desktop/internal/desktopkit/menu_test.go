package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// eachKit exercises the single BoardUI design system in both color schemes.
func eachKit(t *testing.T, f func(t *testing.T, k Kit)) {
	t.Helper()
	for _, dark := range []bool{false, true} {
		name := "boardui/light"
		if dark {
			name = "boardui/dark"
		}
		t.Run(name, func(t *testing.T) { f(t, Kit{Dark: dark}) })
	}
}

func testMenuItems() []MenuEntry {
	return []MenuEntry{
		{ID: "copy", Label: "Copy", Icon: IconCopy, Shortcut: "⌘C"},
		{ID: "off", Label: "Disabled", Disabled: true},
		MenuSeparator("s1"),
		{ID: "merge", Label: "Merge into", Children: []MenuEntry{{ID: "tagA", Label: "Tag A", Swatch: "blue"}}},
		{ID: "del", Label: "Delete", Destructive: true, Count: "3"},
	}
}

func scenarioCompositionMenuChoose(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		var got string
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				if _, id := k.Menu(c, MenuProps{Label: "Actions", Items: testMenuItems(), Count: 2}); id != "" {
					got = id
				}
			})
		}
		tt := compositionTester(t, view, 600, 500)
		if err := tt.Click("Actions"); err != nil {
			t.Fatal(err)
		}

		for _, s := range []string{"Copy", "Disabled", "Merge into", "Delete"} {
			if !tt.HasText(s) {
				t.Fatalf("menu item %q not shown: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Disabled"); err != nil || got != "" {
			t.Fatalf("disabled item chosen: %q %v", got, err)
		}
		if err := tt.Click("Delete"); err != nil {
			t.Fatal(err)
		}
		if got != "del" {
			t.Fatalf("chosen = %q", got)
		}
		if tt.HasText("Copy") {
			t.Fatal("menu did not close after choosing")
		}
	})
}

func scenarioCompositionMenuKeyboardAndEscape(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		var got string
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				if _, id := k.Menu(c, MenuProps{Label: "More", IconOnly: true, Icon: IconMore, Items: testMenuItems()}); id != "" {
					got = id
				}
			})
		}
		tt := compositionTester(t, view, 600, 500)
		if err := tt.Click("More"); err != nil {
			t.Fatal(err)
		}

		// The in-window menu retains native focus and keyboard tracking.

		tt.Key(0, ui.KeyEscape)
		if tt.HasText("Copy") {
			t.Fatal("Escape did not close the menu")
		}
		tt.Key(0, ui.KeyEnter) // the trigger keeps focus; Enter reopens it
		if !tt.HasText("Copy") {
			t.Fatal("Enter did not reopen the menu")
		}
		tt.Key(0, ui.KeyEnter) // first item highlighted by the keyboard
		if got != "copy" {
			t.Fatalf("keyboard choice = %q", got)
		}
	})
}

func scenarioCompositionContextMenu(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		var got string
		view := func(c *ui.Context) {
			k.Apply(c)
			got2 := k.ContextMenu(c, ContextMenuProps{Items: testMenuItems()}, func() {
				ui.Text(c, "Row").Padding(20)
			})
			if got2 != "" {
				got = got2
			}
		}
		tt := compositionTester(t, view, 600, 500)
		if err := tt.RightClick("Row"); err != nil {
			t.Fatal(err)
		}

		if !tt.HasText("Copy") {
			t.Fatal("context menu not shown")
		}
		if err := tt.Click("Copy"); err != nil {
			t.Fatal(err)
		}
		if got != "copy" {
			t.Fatalf("chosen = %q", got)
		}
	})
}
