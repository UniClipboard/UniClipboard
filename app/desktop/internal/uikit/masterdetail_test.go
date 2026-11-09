package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestMasterDetail(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		key, clicks := "a", 0
		view := func(c *ui.Context) {
			k.Apply(c)
			k.MasterDetail(c, MasterDetailProps{DetailKey: key, DetailLabel: "Device detail"},
				func() {
					if k.Button(c.Key("pick"), "Pick", ButtonProps{}).Clicked() {
						clicks++
					}
				},
				func() { k.Text(c, "Detail "+key, TextProps{}) })
		}
		tt := ui.NewTester(view, 800, 600)
		if !tt.HasText("Detail a") {
			t.Fatal("detail missing")
		}
		if err := tt.Click("Pick"); err != nil || clicks != 1 {
			t.Fatalf("click: %v %d", err, clicks)
		}
		key = "b"
		tt.Frame()
		if !tt.HasText("Detail b") || tt.HasText("Detail a") {
			t.Fatalf("detail not replaced: %v", tt.Texts())
		}
	})
}
