package desktopkit

import (
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionToolbarButtons(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		sent, pinned := 0, 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				k.Toolbar(c, "Item actions", func() {
					k.ToolbarGroup(c, func() {
						if k.ToolbarButton(c.Key("send"), "Send", ToolbarButtonProps{Variant: Primary, Icon: IconSend}).Clicked() {
							sent++
						}
						if k.ToolbarButton(c.Key("pin"), "", ToolbarButtonProps{Icon: IconPin, Label: "Pin"}).Clicked() {
							pinned++
						}
					})
					k.ToolbarDivider(c)
					if k.ToolbarButton(c.Key("off"), "Disabled", ToolbarButtonProps{Disabled: true}).Clicked() {
						t.Error("disabled toolbar button clicked")
					}
				})
			})
		}
		tt := compositionTester(t, view, 600, 300)
		if err := tt.Click("Send"); err != nil || sent != 1 {
			t.Fatalf("send: %v %d", err, sent)
		}
		_ = tt.Click("Disabled")
		if _, ok := tt.AccessByRole(ui.RoleToolbar, "Item actions"); !ok {
			t.Fatal("toolbar not exposed with its label")
		}
		// roving focus: Right moves to the next item, Space presses it.
		_ = tt.Click("Send")
		tt.Key(0, ui.KeyRight)
		tt.Key(0, ui.KeySpace)
		if pinned != 1 {
			t.Fatalf("keyboard roving + activation failed: pinned=%d", pinned)
		}
	})

	t.Run("QuietButtons", func(t *testing.T) {
		eachKit(t, func(t *testing.T, k Kit) {
			backClicks, refreshClicks := 0, 0
			busy := false
			view := func(c *ui.Context) {
				k.Apply(c)
				ui.Row(c).Padding(20).Gap(8).AlignItems(ui.Center).Children(func() {
					if k.ToolbarIconButton(c.Key("back"), ToolbarIconButtonProps{
						Icon:    IconBack,
						Label:   "Back",
						Tooltip: "Back",
					}).Clicked() {
						backClicks++
					}
					if k.ToolbarQuietButton(c.Key("refresh"), "Refresh", ToolbarQuietButtonProps{
						Icon:     IconRetry,
						Busy:     busy,
						Disabled: busy,
					}).Clicked() {
						refreshClicks++
					}
				})
			}
			tt := compositionTester(t, view, 400, 120)
			backInfo, ok := tt.AccessByRole(ui.RoleButton, "Back")
			if !ok {
				t.Fatal("back ToolbarIconButton missing")
			}
			refreshInfo, ok := tt.AccessByRole(ui.RoleButton, "Refresh")
			if !ok {
				t.Fatal("refresh ToolbarQuietButton missing")
			}
			if backInfo.Bounds.H != 28 || backInfo.Bounds.W != 28 {
				t.Fatalf("back bounds=%+v, want 28x28", backInfo.Bounds)
			}
			if refreshInfo.Bounds.H != 28 || refreshInfo.Bounds.W <= 28 {
				t.Fatalf("refresh bounds=%+v, want height 28 and width > 28", refreshInfo.Bounds)
			}
			if err := tt.Click("Back"); err != nil || backClicks != 1 {
				t.Fatalf("click back: err=%v clicks=%d", err, backClicks)
			}
			if err := tt.Click("Refresh"); err != nil || refreshClicks != 1 {
				t.Fatalf("click refresh: err=%v clicks=%d", err, refreshClicks)
			}
			busy = true
			tt.Advance(20 * time.Millisecond)
			_ = tt.Click("Refresh")
			if refreshClicks != 1 {
				t.Fatalf("busy quiet button should not activate: clicks=%d", refreshClicks)
			}
		})
	})
}
