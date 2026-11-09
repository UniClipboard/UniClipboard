package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestTransfer(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		half := 50.0
		props := TransferStatusProps{
			Label: "Delivery to Phone", Title: "Phone", Status: "Sending", Summary: "1 of 2 files",
			Tone: StatusInfo, Value: &half, Retryable: true,
			Items: []TransferStatusItem{
				{ID: "1", Label: "a.png", Status: "Done", Tone: StatusPositive, Bytes: "1 MB"},
				{ID: "2", Label: "b.zip", Status: "Sending", Tone: StatusInfo, Value: &half, Bytes: "5 of 10 MB"},
				{ID: "3", Label: "c.txt", Status: "Failed", Tone: StatusCritical, Reason: "Disk full"},
			},
		}
		retries := 0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Row(c).Padding(40).Children(func() {
				if k.TransferStatus(c, props).Retry {
					retries++
				}
			})
		}
		tt := ui.NewTester(view, 700, 500)
		if tt.HasText("b.zip") {
			t.Fatal("details open before hover")
		}
		rect, ok := tt.Find("Delivery to Phone")
		_ = rect
		if err := tt.Click("Delivery to Phone"); err != nil || retries != 1 {
			t.Fatalf("retry click: %v %d (found=%v)", err, retries, ok)
		}
		tt.Frame()
		for _, s := range []string{"Phone", "1 of 2 files", "b.zip", "5 of 10 MB", "Disk full"} {
			if !tt.HasText(s) {
				t.Fatalf("details missing %q: %v", s, tt.Texts())
			}
		}
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		if tt.HasText("b.zip") {
			t.Fatal("Escape did not close the details")
		}

		// Disabled: click never retries.
		props.Disabled = true
		tt = ui.NewTester(view, 700, 500)
		_ = tt.Click("Delivery to Phone")
		if retries != 1 {
			t.Fatalf("disabled retried: %d", retries)
		}

		// Every tone draws an indicator.
		for _, tone := range []StatusTone{StatusNeutral, StatusPositive, StatusInfo, StatusWarning, StatusCritical} {
			ui.NewTester(func(c *ui.Context) {
				k.Apply(c)
				k.TransferIndicator(c, tone, nil, "x")
				k.TransferIndicator(c.Key(1), tone, &half, "x")
			}, 100, 100).Frame()
		}
	})
}
