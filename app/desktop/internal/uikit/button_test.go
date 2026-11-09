package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestButton(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var p ButtonProps
		n := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Row(c).Padding(20).Gap(8).Children(func() {
				if k.Button(c.Key("a"), "Go", p).Clicked() {
					n++
				}
				k.Button(c.Key("b"), "", ButtonProps{Icon: IconAdd, Label: "Add", Size: SizeSmall})
			})
		}
		for _, v := range []ButtonVariant{Standard, Primary, Subtle, Destructive, Link} {
			p = ButtonProps{Variant: v}
			n = 0
			tt := ui.NewTester(view, 300, 120)
			if err := tt.Click("Go"); err != nil || n != 1 {
				t.Fatalf("variant %v click: %v n=%d", v, err, n)
			}
		}
		if _, ok := ui.NewTester(view, 300, 120).Access("Add"); !ok {
			t.Fatal("icon-only button has no accessible name")
		}
		for _, q := range []ButtonProps{{Disabled: true}, {Busy: true}} {
			p, n = q, 0
			tt := ui.NewTester(view, 300, 120)
			_ = tt.Click("Go")
			if n != 0 {
				t.Fatalf("%+v clicked", q)
			}
		}
	})
}

func TestButtonRowRemove(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		n := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			k.ButtonRow(c, func() {
				k.Button(c.Key("a"), "One", ButtonProps{})
				if k.RemoveButton(c.Key("r"), "Remove x").Clicked() {
					n++
				}
			})
		}
		tt := ui.NewTester(view, 300, 100)
		if !tt.HasText("One") {
			t.Fatal("missing button")
		}
		if err := tt.Click("Remove x"); err != nil || n != 1 {
			t.Fatalf("remove: %v %d", err, n)
		}
	})
}
