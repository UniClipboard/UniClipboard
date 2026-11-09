package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestSettings(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		clicks := 0
		tt := ui.NewTester(func(c *ui.Context) {
			k.Apply(c)
			k.SettingsContent(c, SettingsContentProps{Label: "General"}, func() {
				k.SettingsGroup(c.Key("g"), SettingsGroupProps{Title: "Sync"}, func() {
					k.SettingsRow(c.Key("a"), SettingsRowProps{Label: "Auto sync", Description: "Send copies automatically"},
						func() { k.Text(c, "Beta", TextProps{}) },
						func() {
							if k.Button(c, "Configure", ButtonProps{}).Clicked() {
								clicks++
							}
						})
					k.SettingsRow(c.Key("b"), SettingsRowProps{}, nil, func() { k.Text(c, "Bare control", TextProps{}) })
				})
			})
		}, 800, 500)
		for _, s := range []string{"Sync", "Auto sync", "Send copies automatically", "Beta", "Bare control"} {
			if !tt.HasText(s) {
				t.Fatalf("missing %q: %v", s, tt.Texts())
			}
		}
		if _, ok := tt.Access("General"); !ok {
			t.Fatal("settings region not named")
		}
		if err := tt.Click("Configure"); err != nil || clicks != 1 {
			t.Fatalf("control: %v %d", err, clicks)
		}
	})
}
