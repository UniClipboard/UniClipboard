package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestAlertDialogConfirmCancel(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		open := true
		suppress := false
		field := ""
		var res AlertResult
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Children(func() {
				ui.Text(c, "behind")
				r := k.AlertDialog(c, AlertDialogProps{
					Open: &open, Title: "Reset?", Description: "Cannot be undone",
					ConfirmLabel: "Reset", CancelLabel: "Keep", Destructive: true,
					Suppression: &AlertSuppression{Label: "Do not ask again", Checked: &suppress},
					Field:       &AlertField{Label: "Type RESET", Value: &field},
					Error:       "Failed",
				}, func() { ui.Text(c, "Extra body") })
				if r.Confirmed || r.Cancelled {
					res = r
				}
			})
		}
		tt := ui.NewTester(view, 700, 600)
		for _, s := range []string{"Reset?", "Cannot be undone", "Extra body", "Failed", "Keep"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if err := tt.Click("Do not ask again"); err != nil || !suppress {
			t.Fatalf("suppression: %v %v", err, suppress)
		}
		if err := tt.Click("Reset"); err != nil || !res.Confirmed || open {
			t.Fatalf("confirm: %v %+v open=%v", err, res, open)
		}
		open, res = true, AlertResult{}
		tt.Frame()
		if err := tt.Click("Keep"); err != nil || !res.Cancelled || open {
			t.Fatalf("cancel: %v %+v open=%v", err, res, open)
		}
	})
}

func TestAlertDialogEscapeAndBusy(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		open, busy := true, ""
		view := func(c *ui.Context) {
			k.Apply(c)
			k.AlertDialog(c, AlertDialogProps{
				Open: &open, Title: "T", ConfirmLabel: "OK", CancelLabel: "No", BusyLabel: busy,
				ConfirmDisabled: false,
			}, nil)
		}
		tt := ui.NewTester(view, 700, 600)
		busy = "Working"
		tt.Frame()
		tt.Key(0, ui.KeyEscape)
		tt.Frame()
		if !open || !tt.HasText("Working") {
			t.Fatal("busy alert was dismissed")
		}
		busy = ""
		tt.Frame()
		tt.Key(0, ui.KeyEscape)
		if open {
			t.Fatal("Escape did not close the idle alert")
		}
	})
}

func TestAlertDialogConfirmDisabled(t *testing.T) {
	eachKit(t, func(t *testing.T, k Kit) {
		open := true
		var res AlertResult
		view := func(c *ui.Context) {
			k.Apply(c)
			if r := k.AlertDialog(c, AlertDialogProps{Open: &open, Title: "T", ConfirmLabel: "OK",
				CancelLabel: "No", ConfirmDisabled: true}, nil); r.Confirmed {
				res = r
			}
		}
		tt := ui.NewTester(view, 700, 600)
		_ = tt.Click("OK")
		if res.Confirmed || !open {
			t.Fatal("disabled confirm acted")
		}
	})
}
