package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionButton(t *testing.T) {
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
			tt := compositionTester(t, view, 300, 120)
			if err := tt.Click("Go"); err != nil || n != 1 {
				t.Fatalf("variant %v click: %v n=%d", v, err, n)
			}
		}
		if _, ok := compositionTester(t, view, 300, 120).Access("Add"); !ok {
			t.Fatal("icon-only button has no accessible name")
		}
		for _, q := range []ButtonProps{{Disabled: true}, {Busy: true}, {Variant: Subtle, Disabled: true}, {Variant: Subtle, Busy: true}} {
			p, n = q, 0
			tt := compositionTester(t, view, 300, 120)
			_ = tt.Click("Go")
			if n != 0 {
				t.Fatalf("%+v clicked", q)
			}
		}

		// Subtle buttons must stay transparent at rest (no BoardUI blue ghost fill)
		// and use quiet compact geometry (28x28 icon-only, 28 small, 32 medium).
		subtleView := func(c *ui.Context) {
			k.Apply(c)
			ui.Row(c).Fill().Padding(20).Gap(8).AlignItems(ui.Center).Background(k.WorkspaceSurface()).Children(func() {
				k.Button(c.Key("icon"), "", ButtonProps{Variant: Subtle, Icon: IconSettings, Label: "IconSubtle"})
				k.Button(c.Key("small"), "SmallSubtle", ButtonProps{Variant: Subtle, Size: SizeSmall, Icon: IconCopy})
				k.Button(c.Key("med"), "MedSubtle", ButtonProps{Variant: Subtle})
			})
		}
		stt := compositionTester(t, subtleView, 420, 120)
		im := stt.Image()
		bg := im.RGBAAt(4, 4)
		abs := func(v int) int {
			if v < 0 {
				return -v
			}
			return v
		}
		for _, tc := range []struct {
			name   string
			w, h   float32
			square bool
		}{
			{"IconSubtle", 28, 28, true},
			{"SmallSubtle", 0, 28, false},
			{"MedSubtle", 0, 32, false},
		} {
			info, ok := stt.AccessByRole(ui.RoleButton, tc.name)
			if !ok {
				t.Fatalf("subtle button %q missing", tc.name)
			}
			if info.Bounds.H != tc.h || (tc.square && info.Bounds.W != tc.w) {
				t.Fatalf("subtle button %q bounds=%+v, want h=%v (square=%v)", tc.name, info.Bounds, tc.h, tc.square)
			}
			px := im.RGBAAt(int(info.Bounds.X+2), int(info.Bounds.Y+2))
			diff := abs(int(px.R)-int(bg.R)) + abs(int(px.G)-int(bg.G)) + abs(int(px.B)-int(bg.B))
			if diff > 12 {
				t.Fatalf("subtle button %q corner pixel=%v differs from bg=%v (diff=%d); expected transparent rest surface", tc.name, px, bg, diff)
			}
		}
	})
}

func scenarioCompositionButtonRowRemove(t *testing.T) {
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
		tt := compositionTester(t, view, 300, 100)
		if !tt.HasText("One") {
			t.Fatal("missing button")
		}
		if err := tt.Click("Remove x"); err != nil || n != 1 {
			t.Fatalf("remove: %v %d", err, n)
		}
	})
}
