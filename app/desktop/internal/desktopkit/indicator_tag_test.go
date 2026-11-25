package desktopkit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func scenarioCompositionTagNoticeDisclosure(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var removed, acted, toastActed, dismissed int
		open := false
		var toasts ToastStore
		toasts.Warning("Restart needed", ToastOptions{
			ID:       "restart",
			Duration: ToastPersistent,
			Actions: []ToastAction{
				{Label: "Restart now", KeepOpen: true, OnClick: func() { toastActed++ }},
				{Label: "Busy restart", Busy: true, Disabled: true, OnClick: func() { toastActed++ }},
			},
			OnDismiss: func() { dismissed++ },
		})
		toasts.Error("Sync failed", ToastOptions{
			ID:          "err",
			Description: "Please retry",
			Duration:    ToastPersistent,
		})
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Fill().Padding(20).Gap(8).Children(func() {
				if k.Tag(c.Key("t1"), "fin", TagProps{Color: "blue", RemoveLabel: "Remove fin"}).Removed {
					removed++
				}
				if k.Tag(c.Key("t2"), "dock", TagProps{Color: "green", Partial: true, Detail: "2/3", ActionLabel: "Add dock"}).Activated {
					acted++
				}
				k.Disclosure(c.Key("d"), "More", &open, func() { ui.Text(c, "Hidden body") })
			})
			k.Toaster(c, &toasts, ToasterProps{CloseLabel: "Close"})
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
		if !tt.HasText("Restart needed") || !tt.HasText("Sync failed") {
			t.Fatalf("toasts not shown: %v", tt.Texts())
		}
		if err := tt.Click("Restart now"); err != nil || toastActed != 1 {
			t.Fatalf("toast action: %v %d", err, toastActed)
		}
		_ = tt.Click("Busy restart")
		if toastActed != 1 {
			t.Fatalf("disabled/busy toast action fired: %d", toastActed)
		}
		if err := tt.Click("Close"); err != nil || dismissed != 1 {
			t.Fatalf("toast close: %v %d", err, dismissed)
		}
	})
}
