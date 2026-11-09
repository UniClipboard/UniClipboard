package uikit

import (
	"testing"

	"github.com/egoist/mygo/ui"
)

func TestToggleControls(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var tg, sw, cb bool
		val := 5.0
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Gap(8).Children(func() {
				k.ToggleButton(c.Key("t"), &tg, "Tog", ToggleButtonProps{})
				k.Switch(c.Key("s"), &sw, SwitchProps{Label: "Sw"})
				k.Checkbox(c.Key("c"), &cb, CheckboxProps{Label: "Cb"})
				k.Slider(c.Key("l"), &val, SliderProps{Label: "Sl", Min: 0, Max: 10, Step: 1})
			})
		}
		tt := ui.NewTester(view, 300, 300)
		for name, p := range map[string]*bool{"Tog": &tg, "Sw": &sw, "Cb": &cb} {
			if err := tt.Click(name); err != nil || !*p {
				t.Fatalf("%s click: %v %v", name, err, *p)
			}
			tt.Key(0, ui.KeySpace)
			if *p {
				t.Fatalf("%s space did not toggle off", name)
			}
		}
		_ = tt.Click("Sl")
		before := val
		tt.Key(0, ui.KeyRight)
		if val <= before {
			t.Fatalf("slider right: %v -> %v", before, val)
		}
	})
}

func TestToggleDisabled(t *testing.T) {
	layEach(t, func(t *testing.T, k Kit) {
		var sw, cb, tg bool
		view := func(c *ui.Context) {
			k.Apply(c)
			ui.Column(c).Padding(20).Gap(8).Children(func() {
				k.ToggleButton(c.Key("t"), &tg, "Tog", ToggleButtonProps{Disabled: true})
				k.Switch(c.Key("s"), &sw, SwitchProps{Label: "Sw", Disabled: true})
				k.Checkbox(c.Key("c"), &cb, CheckboxProps{Label: "Cb", Disabled: true})
			})
		}
		tt := ui.NewTester(view, 300, 200)
		for _, n := range []string{"Tog", "Sw", "Cb"} {
			_ = tt.Click(n)
		}
		if sw || cb || tg {
			t.Fatal("disabled control toggled")
		}
	})
}
