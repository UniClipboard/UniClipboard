package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionTagNoticeDisclosure(t *testing.T) {
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
				if k.InlineNotice(c.Key("title-only"), NoticeProps{Tone: ToneWarning, Title: "Restart needed", ActionLabel: "Restart now", ActionIcon: IconRetry}).Acted {
					notAct++
				}
				if k.InlineNotice(c.Key("busy-action"), NoticeProps{Tone: ToneWarning, Title: "Restarting", ActionLabel: "Busy restart", ActionBusy: true, ActionDisabled: true}).Acted {
					notAct++
				}
				k.Disclosure(c.Key("d"), "More", &open, func() { ui.Text(c, "Hidden body") })
			})
		}
		tt := compositionTester(t, view, 600, 700)
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
		beforeTitleOnly := notAct
		if err := tt.Click("Restart now"); err != nil || notAct != beforeTitleOnly+1 {
			t.Fatalf("title-only notice action: %v %d", err, notAct)
		}
		_ = tt.Click("Busy restart")
		if notAct != beforeTitleOnly+1 {
			t.Fatalf("disabled/busy notice action fired: %d", notAct)
		}
		if notAct == 0 || dismissed == 0 {
			t.Fatalf("notice buttons: %d %d", notAct, dismissed)
		}
	})
}
