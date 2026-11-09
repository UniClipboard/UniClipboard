package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

// layEach runs fn for every skin x colour scheme.
func layEach(t *testing.T, fn func(t *testing.T, k Kit)) {
	t.Helper()
	for _, s := range []SkinID{Linux, MacOS, Windows} {
		for _, d := range []bool{false, true} {
			k := Kit{Skin: s, Dark: d}
			name := string(s) + "/light"
			if d {
				name = string(s) + "/dark"
			}
			t.Run(name, func(t *testing.T) { fn(t, k) })
		}
	}
}

func TestSplitView(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		hidden, detail := false, false
		toggled := 0
		view := func(c *ui.Context) {
			var r SplitViewResult
			k.UiRoot(c, func() {
				r = k.SplitView(c, SplitViewProps{
					Title: "History", Subtitle: "21 items",
					Sidebar:            func() { k.Text(c, "SidebarBody", TextProps{}) },
					SidebarHidden:      &hidden,
					SidebarToggleLabel: "Toggle sidebar",
					Search:             func() { k.Text(c, "SearchSlot", TextProps{}) },
					Toolbar:            func() { k.Text(c, "ToolbarSlot", TextProps{}) },
					DetailOpen:         &detail, BackLabel: "Back",
				}, func() {
					k.Pane(c.Key("l"), PaneProps{Kind: PaneList, Label: "List", DetailOpen: &detail}, func() { k.Text(c, "ListBody", TextProps{}) })
					k.Pane(c.Key("d"), PaneProps{Kind: PaneDetail, Label: "Detail", DetailOpen: &detail}, func() { k.Text(c, "DetailBody", TextProps{}) })
				})
			})
			if r.SidebarToggled {
				toggled++
			}
		}
		tt := ui.NewTester(view, 1000, 600)
		for _, s := range []string{"History", "SidebarBody", "SearchSlot", "ToolbarSlot", "ListBody", "DetailBody"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Toggle sidebar"); err != nil {
			t.Fatal(err)
		}
		tt.Advance(1e9)
		if !hidden || toggled == 0 {
			t.Fatalf("toggle: hidden=%v toggled=%d", hidden, toggled)
		}
		if tt.HasText("SidebarBody") {
			t.Fatal("sidebar still built after hiding")
		}

		// Narrow window: one pane at a time, Back returns to the list.
		tt = ui.NewTester(view, 600, 600)
		if !tt.HasText("ListBody") || tt.HasText("DetailBody") {
			t.Fatalf("narrow list: %v", tt.Texts())
		}
		detail = true
		tt.Frame()
		if tt.HasText("ListBody") || !tt.HasText("DetailBody") {
			t.Fatalf("narrow detail: %v", tt.Texts())
		}
		if err := tt.Click("Back"); err != nil {
			t.Fatal(err)
		}
		if detail {
			t.Fatal("back did not close the detail")
		}
	})
}

func TestSplitViewOverlayEscape(t *testing.T) {
	k := Kit{Skin: MacOS}
	hidden := false
	view := func(c *ui.Context) {
		k.UiRoot(c, func() {
			k.SplitView(c, SplitViewProps{
				Title: "T", Sidebar: func() { k.Text(c, "SB", TextProps{}) },
				SidebarHidden: &hidden, SidebarOverlay: true, SidebarToggleLabel: "Toggle",
			}, nil)
		})
	}
	tt := ui.NewTester(view, 900, 500)
	tt.Key(0, ui.KeyEscape)
	if !hidden {
		t.Fatal("Escape did not dismiss the overlay sidebar")
	}
}
