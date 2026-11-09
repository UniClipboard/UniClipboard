package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestDetailHeader(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var pin, del int
		view := func(c *ui.Context) {
			k.Apply(c)
			r := k.DetailHeader(c, DetailHeaderProps{PinLabel: "Pin", DeleteLabel: "Delete"}, func() { k.Text(c, "Meta", TextProps{}) })
			if r.PinClicked {
				pin++
			}
			if r.DeleteClicked {
				del++
			}
		}
		tt := ui.NewTester(view, 600, 200)
		if !tt.HasText("Meta") {
			t.Fatal("body missing")
		}
		if k.Skin != Windows {
			if _, ok := tt.Access("Pin"); ok {
				t.Fatal("pin must only be drawn on Windows")
			}
			return
		}
		if err := tt.Click("Pin"); err != nil || pin != 1 {
			t.Fatalf("pin: %v %d", err, pin)
		}
		if err := tt.Click("Delete"); err != nil || del != 1 {
			t.Fatalf("delete: %v %d", err, del)
		}
	})
}

func TestEntityHeader(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		clicks := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			k.EntityHeader(c, EntityHeaderProps{
				Icon: IconDesktop, Title: "Office PC", Badge: "This device",
				Status:      EntityStatus{Text: "Online", Tone: StatusPositive, Detail: "LAN"},
				Description: "Paired yesterday",
			}, func() {
				if k.Button(c, "Unpair", ButtonProps{Variant: Destructive}).Clicked() {
					clicks++
				}
			})
		}
		tt := ui.NewTester(view, 700, 200)
		for _, s := range []string{"Office PC", "This device", "Online", "LAN", "Paired yesterday"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Unpair"); err != nil || clicks != 1 {
			t.Fatalf("unpair: %v %d", err, clicks)
		}
	})
}
