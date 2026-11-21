package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionActionBar(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		n := 0
		status := ""
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Fill().Children(func() {
				k.ActionBar(c, ActionBarProps{
					Status:   status,
					Floating: true,
				}, func() {
					if k.ActionDockPrimary(c.Key("copy"), "Copy", IconCopy, "↩").Clicked() {
						n++
						status = "Copied"
					}
					k.ActionDockSeparator(c.Key("sep"))
					_, chosen := k.ActionDockMenu(c.Key("send"), MenuProps{
						Label: "Send to…",
						Icon:  IconSend,
						Items: []MenuEntry{{ID: "all", Label: "All Devices"}},
					})
					if chosen != "" {
						status = "Sent"
					}
				})
			})
		}
		tt := compositionTester(t, view, 600, 300)
		if !tt.HasText("↩") || !tt.HasText("Send to…") {
			t.Fatal("floating dock controls missing")
		}
		if err := tt.Click("Copy"); err != nil || n != 1 {
			t.Fatalf("click: %v %d", err, n)
		}
		if !tt.HasText("Copied") {
			t.Fatal("status toast missing")
		}
	})
}
