package e2e_test

import (
	"encoding/json"
	"fmt"
	"image/color"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
)

// Exercise authoring, interaction, rendering and state retention through public controls.
func scenarioPaletteEndToEnd(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	brand := boardui.NewPaletteTheme(boardui.Palette{Accent: ui.RGB(180, 20, 190)}, boardui.Palette{Accent: ui.RGB(210, 90, 220)})
	full := boardui.NewPaletteTheme(
		boardui.Palette{Background: ui.RGB(250, 245, 235), Surface: ui.RGB(255, 250, 240), Elevated: ui.RGB(255, 253, 248), Text: ui.RGB(50, 35, 20), TextMuted: ui.RGB(115, 95, 75), Border: ui.RGB(205, 185, 160), Accent: ui.RGB(245, 210, 60), AccentText: ui.RGB(20, 15, 5), Success: ui.RGB(35, 120, 85), Warning: ui.RGB(155, 95, 20), Danger: ui.RGB(175, 40, 65), Overlay: ui.RGBA(40, 25, 10, .65)},
		boardui.Palette{Background: ui.RGB(25, 20, 30), Surface: ui.RGB(40, 30, 45), Elevated: ui.RGB(55, 40, 60), Text: ui.RGB(250, 240, 255), TextMuted: ui.RGB(185, 165, 195), Border: ui.RGB(90, 70, 100), Accent: ui.RGB(240, 210, 80), AccentText: ui.RGB(20, 15, 5), Success: ui.RGB(100, 215, 160), Warning: ui.RGB(230, 170, 65), Danger: ui.RGB(245, 110, 140), Overlay: ui.RGBA(10, 5, 15, .75)})
	overridden, err := full.WithOverrides(map[string]ui.Color{"border-focus-ring": ui.RGB(0, 180, 150), "dialog-backdrop": {}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if full.Color(false, "dialog-backdrop").A == 0 || overridden.Color(false, "dialog-backdrop").A != 0 {
		t.Fatal("exact overrides mutated the parent or lost transparent color")
	}
	if _, err := full.WithOverrides(map[string]ui.Color{"unknown": {}}, nil); err == nil {
		t.Fatal("unknown override accepted")
	}
	if brand.Color(false, "accent-500") != ui.RGB(180, 20, 190) || brand.Color(true, "accent-500") != ui.RGB(210, 90, 220) {
		t.Fatal("independent brand colors lost")
	}
	empty := boardui.NewPaletteTheme(boardui.Palette{}, boardui.Palette{})
	var source *boardui.ColorTheme
	for _, dark := range []bool{false, true} {
		for _, name := range boardui.ColorTokenNames() {
			if empty.Color(dark, name) != source.Color(dark, name) {
				t.Fatalf("empty palette changed %s", name)
			}
		}
		for _, name := range []string{"border-focus-ring", "gradient-button-primary-default-from", "gradient-button-primary-default-to", "button-ghost-background", "button-ghost-hover", "button-ghost-active", "switch-on-chip-start", "switch-on-chip-end", "slider-accent-shadow", "native-selection", "notification-information-background"} {
			if brand.Color(dark, name) == source.Color(dark, name) {
				t.Fatalf("brand state retained source color: %s", name)
			}
		}
	}
	if full.Color(false, "background-dialog") != ui.RGB(255, 253, 248) || full.Color(false, "background-popover") != ui.RGB(255, 253, 248) {
		t.Fatal("elevated base did not reach floating surfaces")
	}
	lightBrand := boardui.NewPaletteTheme(boardui.Palette{Accent: ui.RGB(255, 245, 180)}, boardui.Palette{})
	if lightBrand.Color(false, "text-on-accent") == ui.RGB(255, 255, 255) {
		t.Fatal("light brand retained white foreground")
	}
	if full.Color(false, "text-white") != ui.RGB(255, 255, 255) {
		t.Fatal("accent foreground corrupted literal white")
	}
	k := boardui.Kit{Colors: brand}
	value := "Original"
	enabled := false
	count := 0
	dialogOpen := false
	var observed ui.Theme
	view := func(c *ui.Context) {
		k.Apply(c)
		observed = *c.Theme()
		ui.Column(c).Fill().Padding(24).Gap(12).Background(k.Color("background-full")).Children(func() {
			k.Input(c, &value, boardui.InputProps{Label: "Palette input"})
			if k.Button(c, "Palette save", boardui.ButtonProps{}).Clicked() {
				count++
			}
			k.Button(c, "Delete", boardui.ButtonProps{Variant: "danger"})
			k.Button(c, "Disabled", boardui.ButtonProps{Disabled: true})
			k.Switch(c, &enabled, boardui.SwitchProps{Label: "Palette switch"})
			k.Input(c, &value, boardui.InputProps{Label: "Invalid input", Error: "Example error"})
			if k.Button(c, "Palette dialog", boardui.ButtonProps{Variant: "secondary"}).Clicked() {
				dialogOpen = true
			}
		})
		k.Dialog(c, boardui.DialogProps{Open: &dialogOpen, Title: "Palette overlay", Width: 400}, func() { k.Text(c, "Elevated content", "body-regular") })
	}
	tt := ui.NewTesterWith(view, 640, 560, ui.TesterOptions{})
	tt.SetLocale("en-US")
	mustClick(t, tt, "Palette input")
	tt.Command("selectAll")
	tt.Type("Retained 中文")
	mustClick(t, tt, "Palette switch")
	receipt := map[string]map[string]ui.Color{}
	for index, scene := range []struct {
		name  string
		theme *boardui.ColorTheme
		dark  bool
	}{
		{"brand-light", brand, false}, {"brand-dark", brand, true}, {"full-light", full, false}, {"full-dark", full, true}, {"override-light", overridden, false},
	} {
		k.Colors, k.Dark = scene.theme, scene.dark
		tt.Frame()
		tt.Advance(400 * time.Millisecond)
		if observed.Background != k.Color("background-full") || observed.Accent != k.Color("accent-500") || observed.AccentText != k.Color("text-on-accent") {
			t.Fatal("native primitive palette diverged")
		}
		pixel := color.RGBAModel.Convert(tt.Image().At(0, 0)).(color.RGBA)
		bg := k.Color("background-full")
		if pixel != (color.RGBA{R: bg.R, G: bg.G, B: bg.B, A: bg.A}) {
			t.Fatalf("rendered background ignores palette: %v", pixel)
		}
		if value != "Retained 中文" || !enabled {
			t.Fatal("palette switch reset caller state")
		}
		mustClick(t, tt, "Palette save")
		tt.Advance(400 * time.Millisecond)
		capture(t, tt, fmt.Sprintf("palette-%02d-%s", index, scene.name))
		mustClick(t, tt, "Palette dialog")
		tt.Advance(400 * time.Millisecond)
		mustAccess(t, tt, ui.RoleDialog, "Palette overlay")
		capture(t, tt, fmt.Sprintf("palette-%02d-%s-dialog", index, scene.name))
		tt.Key(0, ui.KeyEscape)
		if dialogOpen {
			t.Fatal("themed dialog did not dismiss")
		}
		receipt[scene.name] = scene.theme.Tokens(scene.dark)
	}
	if count != 5 {
		t.Fatalf("activation count %d", count)
	}
	data, err := json.MarshalIndent(receipt, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"), "palette-tokens.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
}
