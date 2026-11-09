package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSourceList(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		st := SourceState{}
		on, bad := StatusPositive, StatusCritical
		extra := false
		props := SourceListProps{Label: "Devices", Footer: "2 online", Header: func() {},
			Sections: []SourceListSection{
				{ID: "paired", Title: "Paired", Action: &SourceListAction{Label: "Refresh", Icon: IconUpdate},
					Items: []SourceListItem{
						{ID: "a", Title: "Phone", Subtitle: "Online", Icon: IconPhone, Status: &on},
						{ID: "b", Title: "Laptop", Subtitle: "Key changed", SubtitleTone: &bad, Icon: IconDesktop, Dimmed: true},
					}, Extra: func() { extra = true }},
				{ID: "inv", Title: "Invitations", Placeholder: "Invite Device",
					Items: nil},
			}}
		var kind SourceCommandKind
		var sec string
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SourceList(c, &st, props).Height(400)
			if kd, s := st.Command(); kd != SourceNoCommand {
				kind, sec = kd, s
			}
		}, 400, 500)
		for _, s := range []string{"Phone", "Key changed", "2 online", "Invite Device"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if !extra {
			t.Fatal("extra not built")
		}
		if err := tt.Click("Laptop"); err != nil || st.Value != "b" {
			t.Fatalf("click: %v %q", err, st.Value)
		}
		tt.Key(0, ui.KeyUp)
		if st.Value != "a" {
			t.Fatalf("up: %q", st.Value)
		}
		tt.Key(0, ui.KeyEnd)
		if st.Value != "b" {
			t.Fatalf("end: %q", st.Value)
		}
		if err := tt.Click("Invite Device"); err != nil || kind != SourcePlaceholder || sec != "inv" {
			t.Fatalf("placeholder: %v %v %v", err, kind, sec)
		}
		if err := tt.Click("Refresh"); err != nil || kind != SourceSectionAction || sec != "paired" {
			t.Fatalf("action: %v %v %v", err, kind, sec)
		}
	})
}

func TestSourceListDisabledAction(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		st := SourceState{}
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SourceList(c, &st, SourceListProps{Label: "D", Sections: []SourceListSection{
				{ID: "s", Title: "S", Action: &SourceListAction{Label: "Refresh", Icon: IconUpdate, Disabled: true}}}}).Height(200)
		}, 300, 300)
		_ = tt.Click("Refresh")
		if kd, _ := st.Command(); kd != SourceNoCommand {
			t.Fatal("disabled action fired")
		}
	})
}
