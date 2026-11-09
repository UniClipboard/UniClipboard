package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestStepList(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		clicked := ""
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			r := k.StepList(c, StepListProps{Label: "Setup", Steps: []StepListStep{
				{ID: "s1", Title: "Name this device", Description: "Pick a name", Done: true},
				{ID: "s2", Title: "Pair a device", Status: &StepStatus{Text: "Waiting for your first copy", Active: true},
					Action: &StepAction{Label: "Pair a Device", Primary: true}},
				{ID: "s3", Title: "Copy something"},
			}})
			if r.Clicked != "" {
				clicked = r.Clicked
			}
		}, 500, 500)
		for _, s := range []string{"Name this device", "Waiting for your first copy", "3", "Pair a Device"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if tt.HasText("1") {
			t.Fatal("done step should show a check, not its number")
		}
		if err := tt.Click("Pair a Device"); err != nil || clicked != "s2" {
			t.Fatalf("action: %v %q", err, clicked)
		}
	})
}
