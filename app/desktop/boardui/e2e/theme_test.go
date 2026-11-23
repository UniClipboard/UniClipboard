package e2e_test

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui"
	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

// Exercise the shared theme through real public controls and application composition.
func scenarioThemeEndToEnd(t *testing.T) {
	if err := boardui.LoadFonts(); err != nil {
		t.Fatal(err)
	}
	light := map[string]ui.Color{"accent-500": ui.RGB(147, 51, 234), "background-full": ui.RGB(250, 245, 255), "text-primary": ui.RGB(59, 7, 100)}
	dark := map[string]ui.Color{"accent-500": ui.RGB(192, 132, 252), "background-full": ui.RGB(30, 10, 45), "text-primary": ui.RGB(243, 232, 255)}
	theme, err := boardui.NewColorTheme(light, dark)
	if err != nil {
		t.Fatal(err)
	}
	light["accent-500"] = ui.RGB(0, 0, 0)
	if _, err := boardui.NewColorTheme(map[string]ui.Color{"invalid-token": {}}, nil); err == nil {
		t.Fatal("unknown override accepted")
	}
	k := desktopkit.Kit{Colors: theme}
	name := "Original"
	clicks := 0
	var observed ui.Theme
	view := func(c *ui.Context) {
		k.Apply(c)
		observed = *c.Theme()
		public := boardui.Kit{Dark: k.Dark, Colors: k.Colors}
		ui.Column(c).Fill().Padding(24).Gap(12).Background(k.Color("background-full")).Children(func() {
			public.Input(c, &name, boardui.InputProps{Label: "Theme input"})
			if public.Button(c, "Save theme", boardui.ButtonProps{}).Clicked() {
				clicks++
			}
			k.ActionBar(c, desktopkit.ActionBarProps{Status: "Saved"}, func() { public.Text(c, name, "body-regular") })
		})
	}
	tt := ui.NewTesterWith(view, 640, 480, ui.TesterOptions{})
	tt.SetLocale("en-US")
	mustClick(t, tt, "Theme input")
	tt.Command("selectAll")
	tt.Type("Retained 中文")
	receipt := map[string]map[string]ui.Color{}
	for _, isDark := range []bool{false, true, false} {
		k.Dark = isDark
		tt.Frame()
		tt.Advance(400 * time.Millisecond)
		scheme := "light"
		if isDark {
			scheme = "dark"
		}
		wantAccent := ui.RGB(147, 51, 234)
		if isDark {
			wantAccent = dark["accent-500"]
		}
		if observed.Accent != wantAccent || observed.Background != k.Color("background-full") || observed.Text != k.Color("text-primary") {
			t.Fatalf("native theme bypasses overrides: %+v", observed)
		}
		from, _ := (boardui.Kit{Dark: isDark, Colors: theme}).Gradient("button-primary-default")
		if from != wantAccent {
			t.Fatal("primary gradient bypasses accent")
		}
		copy := theme.Tokens(isDark)
		copy["accent-500"] = ui.RGB(0, 0, 0)
		if k.Color("accent-500") != wantAccent {
			t.Fatal("theme snapshot aliases state")
		}
		if name != "Retained 中文" {
			t.Fatal("theme switch reset caller state")
		}
		mustClick(t, tt, "Save theme")
		capture(t, tt, "theme-"+scheme)
		receipt[scheme] = theme.Tokens(isDark)
	}
	if clicks != 3 {
		t.Fatalf("button activation count: %d", clicks)
	}
	k.Colors = nil
	tt.Frame()
	tt.Advance(400 * time.Millisecond)
	if observed.Accent != boardui.Token(false, "accent-500") {
		t.Fatal("default theme was mutated")
	}
	capture(t, tt, "theme-default-restored")
	data, err := json.MarshalIndent(receipt, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"), "theme-tokens.json"), data, 0600); err != nil {
		t.Fatal(err)
	}
}
