package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func navProps(compact bool, collapsed bool) SidebarNavProps {
	online := StatusPositive
	return SidebarNavProps{
		Label: "Main", Compact: compact,
		Sections: []SidebarNavSection{
			{ID: "lib", Items: []SidebarNavItem{
				{ID: "all", Label: "All Items", Icon: IconHistory, Count: "12"},
				{ID: "pins", Label: "Pinned", Icon: IconPin},
			}},
			{ID: "tags", Title: "Tags", Icon: IconTag, Action: &NavAction{Label: "New Tag", Icon: IconAdd},
				Collapse: &NavCollapse{Collapsed: collapsed, Label: "Hide Tags"},
				Items:    []SidebarNavItem{{ID: "t1", Label: "Work", Swatch: "blue"}},
				More:     &NavAction{Label: "All Tags"}},
			{ID: "dev", Title: "Devices", Icon: IconDevices,
				Placeholder: &NavPlaceholder{Text: "No devices", Action: &NavAction{Label: "Add Device"}}},
			{ID: "d2", Title: "Online", Items: []SidebarNavItem{{ID: "ph", Label: "Phone", Status: &online}}},
		},
		Footer: []SidebarNavItem{{ID: "settings", Label: "Settings", Icon: IconSettings, ShortcutHint: "Ctrl+,"}},
	}
}

func TestSidebarNav(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		st := NavState{Value: "all"}
		collapsed := false
		var kind NavCommandKind
		var sec string
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SidebarNav(c, &st, navProps(false, collapsed)).Height(500).Width(240)
			if kd, s := st.Command(); kd != NavNoCommand {
				kind, sec = kd, s
				if kd == NavToggleCollapse {
					collapsed = !collapsed
				}
			}
		}, 300, 600)
		for _, s := range []string{"All Items", "Pinned", "Work", "Phone", "Settings", "No devices", "All Tags"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Pinned"); err != nil {
			t.Fatal(err)
		}
		if st.Value != "pins" {
			t.Fatalf("click: %q", st.Value)
		}
		tt.Key(0, ui.KeyDown)
		if st.Value != "t1" {
			t.Fatalf("down: %q", st.Value)
		}
		tt.Key(0, ui.KeyEnd)
		if st.Value != "settings" {
			t.Fatalf("end: %q", st.Value)
		}
		tt.Key(0, ui.KeyHome)
		if st.Value != "all" {
			t.Fatalf("home: %q", st.Value)
		}
		if err := tt.Click("All Tags"); err != nil || kind != NavMore || sec != "tags" {
			t.Fatalf("more: %v %v %v", err, kind, sec)
		}
		if err := tt.Click("Add Device"); err != nil || kind != NavPlaceholderAct || sec != "dev" {
			t.Fatalf("placeholder: %v %v %v", err, kind, sec)
		}
		if err := tt.Click("Hide Tags"); err != nil || kind != NavToggleCollapse || !collapsed {
			t.Fatalf("collapse: %v %v", err, kind)
		}
		tt.Frame()
		if tt.HasText("Work") {
			t.Fatal("collapsed section still shows items")
		}
	})
}

func TestSidebarNavCompact(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		st := NavState{Value: "all"}
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SidebarNav(c, &st, navProps(true, false)).Height(500)
		}, 200, 600)
		if err := tt.Click("Pinned"); err != nil {
			t.Fatal(err)
		}
		if st.Value != "pins" {
			t.Fatalf("rail click: %q", st.Value)
		}
		if err := tt.Click("Settings"); err != nil || st.Value != "settings" {
			t.Fatalf("footer: %v %q", err, st.Value)
		}
		// 折叠分区的图标打开弹出菜单，选条目。
		if err := tt.Click("Tags"); err != nil {
			t.Fatal(err)
		}
		var err error
		if k.Skin == MacOS {
			err = tt.ChooseMenuItem("Work")
		} else {
			err = tt.Click("Work")
		}
		if err != nil {
			t.Fatal(err)
		}
		if st.Value != "t1" {
			t.Fatalf("flyout pick: %q", st.Value)
		}
	})
}

func TestSidebarNavEntryStatus(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		st := NavState{}
		on := StatusCritical
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SidebarNavEntry(c, &st, SidebarNavItem{ID: "x", Label: "Router", Status: &on, Count: "3"})
		}, 300, 100)
		if err := tt.Click("Router"); err != nil || st.Value != "x" || !st.Changed() {
			t.Fatalf("entry: %v %q", err, st.Value)
		}
	})
}
