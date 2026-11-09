package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTagNoticeDisclosure(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var removed, acted, notAct, dismissed int
		open := false
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Gap(8).Children(func() {
				if k.Tag(c.Key("t1"), "fin", TagProps{Color: "blue", RemoveLabel: "Remove fin"}).Removed {
					removed++
				}
				if k.Tag(c.Key("t2"), "dock", TagProps{Color: "green", Partial: true, Detail: "2/3", ActionLabel: "Add dock"}).Activated {
					acted++
				}
				for _, tone := range []NoticeTone{ToneInfo, ToneSuccess, ToneWarning, ToneError} {
					r := k.InlineNotice(c.Key("n"+string(rune('a'+tone))), NoticeProps{Tone: tone, Title: "T", Text: "body", ActionLabel: "Act", DismissLabel: "Close"})
					if r.Acted {
						notAct++
					}
					if r.Dismissed {
						dismissed++
					}
				}
				k.Disclosure(c.Key("d"), "More", &open, func() { ui.Text(c, "Hidden body") })
			})
		}
		tt := ui.NewTester(view, 600, 700)
		if err := tt.Click("Remove fin"); err != nil || removed != 1 {
			t.Fatalf("remove: %v %d", err, removed)
		}
		if err := tt.Click("Add dock"); err != nil || acted != 1 {
			t.Fatalf("action: %v %d", err, acted)
		}
		if tt.HasText("Hidden body") {
			t.Fatal("disclosure open initially")
		}
		if err := tt.Click("More"); err != nil || !open {
			t.Fatalf("expand: %v %v", err, open)
		}
		tt.Frame()
		if !tt.HasText("Hidden body") {
			t.Fatal("body not shown")
		}
		_ = tt.Click("Act")
		_ = tt.Click("Close")
		if notAct == 0 || dismissed == 0 {
			t.Fatalf("notice buttons: %d %d", notAct, dismissed)
		}
	})
}
