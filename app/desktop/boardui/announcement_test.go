package boardui_test

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

func TestCloseButtonSizesAndInteraction(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	expected := map[string]float32{"2xs": 16, "xs": 20, "sm": 24, "md": 32}
	for _, dark := range []bool{false, true} {
		k := boardui.Kit{Dark: dark}
		clicks := map[string]int{}
		tt := ui.NewTesterWith(func(c *ui.Context) {
			k.Apply(c)
			ui.Row(c).Gap(8).Padding(16).Children(func() {
				for _, size := range []string{"2xs", "xs", "sm", "md"} {
					if k.CloseButton(c.Key(size), boardui.CloseButtonProps{Size: size, Label: "Close " + size}).Clicked() {
						clicks[size]++
					}
				}
				if k.CloseButton(c.Key("disabled"), boardui.CloseButtonProps{Size: "xs", Label: "Disabled close", Disabled: true}).Clicked() {
					clicks["disabled"]++
				}
			})
		}, 300, 100, ui.TesterOptions{Dark: dark})

		for size, want := range expected {
			a, ok := tt.AccessByRole(ui.RoleButton, "Close "+size)
			if !ok {
				t.Fatalf("dark=%v missing close button %s", dark, size)
			}
			if a.Bounds.W != want || a.Bounds.H != want {
				t.Fatalf("dark=%v size=%s bounds=%+v want %vx%v", dark, size, a.Bounds, want, want)
			}
			if err := tt.Click("Close " + size); err != nil {
				t.Fatalf("click %s: %v", size, err)
			}
			if clicks[size] != 1 {
				t.Fatalf("clicks[%s]=%d want 1", size, clicks[size])
			}
		}
		dis, ok := tt.AccessByRole(ui.RoleButton, "Disabled close")
		if !ok || !dis.Disabled {
			t.Fatalf("disabled close button not exposed disabled: %+v", dis)
		}
	}
}

func TestCloseButtonPanicsOnInvalidArgs(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	assertPanics := func(name string, view func(*ui.Context)) {
		t.Helper()
		defer func() {
			if recover() == nil {
				t.Fatalf("%s did not panic", name)
			}
		}()
		ui.NewTester(view, 100, 100)
	}
	k := boardui.Kit{}
	assertPanics("empty label", func(c *ui.Context) {
		k.Apply(c)
		k.CloseButton(c, boardui.CloseButtonProps{Size: "xs"})
	})
	assertPanics("unknown size", func(c *ui.Context) {
		k.Apply(c)
		k.CloseButton(c, boardui.CloseButtonProps{Size: "lg", Label: "Close"})
	})
}

func TestAnnouncementLayoutAndDismissLifecycle(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	for _, dark := range []bool{false, true} {
		k := boardui.Kit{Dark: dark}
		open := true
		actions := 0
		dismissed := 0
		closed := 0
		present := false

		tt := ui.NewTesterWith(func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Width(236).Padding(0).Children(func() {
				res := k.Announcement(c.Key("ann"), boardui.AnnouncementProps{
					Open:        &open,
					Title:       "What's new in v2",
					Description: "Explore the new features and improvements.",
					ActionLabel: "See what's new",
					Dismissible: true,
					CloseLabel:  "Dismiss",
				})
				present = res.Present
				if res.ActionClicked {
					actions++
				}
				if res.Dismissed {
					dismissed++
				}
				if res.Closed {
					closed++
				}
			})
		}, 300, 260, ui.TesterOptions{Dark: dark})

		if !present {
			t.Fatalf("dark=%v announcement not present", dark)
		}
		card, ok := tt.AccessByRole(ui.RoleRegion, "What's new in v2")
		if !ok {
			t.Fatalf("dark=%v announcement region missing", dark)
		}
		if card.Bounds.W != 236 {
			t.Fatalf("dark=%v card width=%v want 236", dark, card.Bounds.W)
		}
		cta, ok := tt.AccessByRole(ui.RoleButton, "See what's new")
		if !ok {
			t.Fatalf("dark=%v CTA button missing", dark)
		}
		// Full width inside 12px horizontal padding + 1px border on each side (236 - 24 = 212).
		if cta.Bounds.H != 32 {
			t.Fatalf("dark=%v CTA height=%v want 32", dark, cta.Bounds.H)
		}
		if cta.Bounds.W < 208 || cta.Bounds.W > 212 {
			t.Fatalf("dark=%v CTA width=%v want ~210-212", dark, cta.Bounds.W)
		}
		closeBtn, ok := tt.AccessByRole(ui.RoleButton, "Dismiss")
		if !ok {
			t.Fatalf("dark=%v close button missing", dark)
		}
		if closeBtn.Bounds.W != 20 || closeBtn.Bounds.H != 20 {
			t.Fatalf("dark=%v close button bounds=%+v want 20x20", dark, closeBtn.Bounds)
		}
		// 1px border + 12px padding inset puts close button 13px from card border edges,
		// aligning vertically with the 20x20 leading icon (CSS padding-box containing block).
		if closeBtn.Bounds.Y != card.Bounds.Y+13 || (card.Bounds.X+card.Bounds.W)-(closeBtn.Bounds.X+closeBtn.Bounds.W) != 13 {
			t.Fatalf("dark=%v close button not at top-right 1px border + 12px inset: card=%+v close=%+v", dark, card.Bounds, closeBtn.Bounds)
		}

		if err := tt.Click("See what's new"); err != nil {
			t.Fatalf("click CTA: %v", err)
		}
		if actions != 1 {
			t.Fatalf("actions=%d want 1", actions)
		}

		if err := tt.Click("Dismiss"); err != nil {
			t.Fatalf("click dismiss: %v", err)
		}
		if dismissed != 1 || open {
			t.Fatalf("dismissed=%d open=%v want 1, false", dismissed, open)
		}
		// Exit animation takes 250ms; card stays present briefly then unmounts.
		tt.Advance(100 * time.Millisecond)
		if !present {
			t.Fatalf("dark=%v card unmounted before exit animation completed", dark)
		}
		tt.Advance(220 * time.Millisecond)
		tt.Frame()
		if present || closed != 1 {
			t.Fatalf("dark=%v after exit: present=%v closed=%d want false, 1", dark, present, closed)
		}
		if _, ok := tt.AccessByRole(ui.RoleRegion, "What's new in v2"); ok {
			t.Fatalf("dark=%v dismissed card still in access tree", dark)
		}
	}
}

func TestAnnouncementReducedMotionAndOptionalParts(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	k := boardui.Kit{}
	open := true
	closed := 0
	present := false

	tt := ui.NewTesterWith(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Width(236).Children(func() {
			res := k.Announcement(c.Key("ann"), boardui.AnnouncementProps{
				Open:        &open,
				Title:       "Title only",
				Dismissible: true,
				IntroDelay:  300 * time.Millisecond,
			})
			present = res.Present
			if res.Closed {
				closed++
			}
		})
	}, 300, 200, ui.TesterOptions{Preferences: ui.Preferences{ReduceMotion: true}})

	if !present {
		t.Fatal("title-only announcement not present")
	}
	if _, ok := tt.AccessByRole(ui.RoleButton, "Dismiss"); !ok {
		t.Fatal("default CloseLabel Dismiss missing")
	}
	if err := tt.Click("Dismiss"); err != nil {
		t.Fatal(err)
	}
	tt.Frame()
	if present || closed != 1 {
		t.Fatalf("reduced-motion dismiss did not unmount immediately: present=%v closed=%d", present, closed)
	}
}

func TestAnnouncementStartsClosedNoLayout(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	k := boardui.Kit{}
	open := false
	present := true
	tt := ui.NewTester(func(c *ui.Context) {
		k.Apply(c)
		ui.Column(c).Width(236).Children(func() {
			res := k.Announcement(c.Key("ann"), boardui.AnnouncementProps{
				Open:        &open,
				Title:       "Hidden",
				ActionLabel: "Open",
				Dismissible: true,
			})
			present = res.Present
		})
	}, 300, 200)
	if present {
		t.Fatal("initially closed announcement reported present")
	}
	if _, ok := tt.AccessByRole(ui.RoleRegion, "Hidden"); ok {
		t.Fatal("initially closed announcement rendered")
	}
}
