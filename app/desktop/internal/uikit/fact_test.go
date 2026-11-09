package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestFactList(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.FactList(c, FactListProps{Label: "Image facts", Facts: []Fact{
				{ID: "d", Label: "Dimensions", Value: "1920 x 1080", Monospace: true},
				{ID: "f", Label: "Format", Value: "PNG"},
			}})
		}, 500, 200)
		for _, s := range []string{"Dimensions", "1920 x 1080", "Format", "PNG"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q", s)
			}
		}
	})
}
