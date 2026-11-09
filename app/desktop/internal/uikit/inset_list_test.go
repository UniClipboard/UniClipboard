package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestInsetList(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		clicked := ""
		custom := false
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			r := k.InsetList(c, InsetListProps{Label: "Delivery", Rows: []InsetListRow{
				{ID: "a", Title: "Phone", Detail: "Delivered", Tone: StatusPositive},
				{ID: "b", Title: "Laptop", Detail: "Failed", Tone: StatusCritical, Action: "Retry"},
				{ID: "c", Title: "Tablet", Status: func() { custom = true; ui.Text(c, "Custom") }},
			}})
			if r.Clicked != "" {
				clicked = r.Clicked
			}
		}, 400, 300)
		for _, s := range []string{"Phone", "Delivered", "Failed", "Custom"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if !custom {
			t.Fatal("custom status not built")
		}
		if err := tt.Click("Retry"); err != nil || clicked != "b" {
			t.Fatalf("action: %v %q", err, clicked)
		}
	})
}
