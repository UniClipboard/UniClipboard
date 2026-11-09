package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// eachKit runs f for the three skins in both colour schemes.
func eachKit(t *testing.T, f func(t *testing.T, k Kit)) {
	t.Helper()
	for _, s := range []SkinID{Linux, MacOS, Windows} {
		for _, d := range []bool{false, true} {
			k := Kit{Skin: s, Dark: d}
			name := string(s) + "/light"
			if d {
				name = string(s) + "/dark"
			}
			t.Run(name, func(t *testing.T) { f(t, k) })
		}
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

func TestMenuChoose(t *testing.T) {
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
		tt := ui.NewTester(view, 600, 500)
		if err := tt.Click("Actions"); err != nil {
			t.Fatal(err)
		}
		if k.Skin == MacOS {
			if len(tt.Menu()) == 0 {
				t.Fatal("system menu missing")
			}
			if err := tt.ChooseMenuItem("Disabled"); err == nil || got != "" {
				t.Fatal("disabled native item was chosen")
			}
			if err := tt.ChooseMenuItem("Delete"); err != nil {
				t.Fatal(err)
			}
			if got != "del" || tt.Menu() != nil {
				t.Fatalf("native choice/close: %q", got)
			}
			return
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

func TestMenuKeyboardAndEscape(t *testing.T) {
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
		tt := ui.NewTester(view, 600, 500)
		if err := tt.Click("More"); err != nil {
			t.Fatal(err)
		}
		if k.Skin == MacOS {
			// Native key tracking belongs to the OS, not the fake host.
			tt.CloseMenu()
			tt.Key(0, ui.KeyEnter)
			if len(tt.Menu()) == 0 {
				t.Fatal("Enter did not request a system menu")
			}
			if err := tt.ChooseMenuItem("Copy"); err != nil {
				t.Fatal(err)
			}
			if got != "copy" {
				t.Fatalf("native choice: %q", got)
			}
			return
		}
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

func TestContextMenu(t *testing.T) {
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
		tt := ui.NewTester(view, 600, 500)
		if err := tt.RightClick("Row"); err != nil {
			t.Fatal(err)
		}
		if k.Skin == MacOS {
			if err := tt.ChooseMenuItem("Copy"); err != nil {
				t.Fatal(err)
			}
			if got != "copy" {
				t.Fatalf("native context choice: %q", got)
			}
			return
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
