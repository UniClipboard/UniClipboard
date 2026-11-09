package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestContentUnavailable(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		clicks := 0
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.ContentUnavailable(c, ContentUnavailableProps{Icon: IconSearch, Title: "No results", Description: "Try another word"}, func() {
				if k.Button(c, "Clear search", ButtonProps{}).Clicked() {
					clicks++
				}
			})
		}, 600, 400)
		if !tt.HasText("No results") || !tt.HasText("Try another word") {
			t.Fatalf("texts: %v", tt.Texts())
		}
		if err := tt.Click("Clear search"); err != nil || clicks != 1 {
			t.Fatalf("action: %v %d", err, clicks)
		}
	})
}
