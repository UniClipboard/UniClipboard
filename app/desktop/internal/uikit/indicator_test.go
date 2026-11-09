package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestIndicators(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		v := 40.0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Gap(8).Children(func() {
				k.ProgressBar(c.Key("p"), ProgressBarProps{Label: "Prog", Value: &v})
				k.ProgressBar(c.Key("p2"), ProgressBarProps{Label: "Indet"})
				k.Badge(c.Key("b"), BadgeProps{Count: 3, Label: "unread"})
				k.ColorDot(c.Key("d"), ColorDotProps{Color: "red", Label: "Dot"})
				for i, kd := range []ContentKind{} {
					_ = i
					_ = kd
				}
				k.KindBadge(c.Key("k"), KindBadgeProps{Label: "Kind", Variant: KindPill})
			})
		}
		tt := ui.NewTester(view, 300, 300)
		for _, n := range []string{"Prog", "Indet", "Dot"} {
			if _, ok := tt.Access(n); !ok {
				t.Errorf("%s not exposed", n)
			}
		}
		if bg, ink := k.TagTint("#336699"); bg.A == 0 || ink.A == 0 {
			t.Fatal("custom tint empty")
		}
	})
}

func TestIconsAndText(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				for name := range iconGlyph {
					k.Icon(c.Key(string(name)), name, 16)
				}
				for _, r := range []TextRole{} {
					_ = r
				}
				k.Text(c, "Hello", TextProps{Emphasized: true})
				k.Text(c.Key("m"), "Mono", TextProps{Monospace: true, Truncate: true})
			})
		}
		tt := ui.NewTester(view, 400, 400)
		if !tt.HasText("Hello") || !tt.HasText("Mono") {
			t.Fatal("text missing")
		}
		for name, g := range iconGlyph {
			if _, ok := glyphs[g]; !ok {
				t.Errorf("icon %q lacks glyph %q", name, g)
			}
		}
	})
}

func TestSurface(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		for _, vib := range []bool{false, true} {
			view := func(c *ui.Context) {
				k.UiRoot(c, func() {
					ui.Row(c).Children(func() {
						k.Surface(c.Key("s"), RegionSidebar).Width(100).Children(func() { ui.Text(c, "Side") })
						k.Surface(c.Key("c"), RegionContent).Grow(1).Children(func() { ui.Text(c, "Main") })
					})
				})
			}
			tt := ui.NewTester(view, 400, 300)
			tt.SetVibrancy(vib)
			tt.Frame()
			if !tt.HasText("Side") || !tt.HasText("Main") {
				t.Fatalf("vibrancy=%v: missing text", vib)
			}
		}
	})
}
