package desktopdemo

import (
	"encoding/json"
	"image/color"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: blur-only capsules with no reactive rim; hover lighting that
// never resets; a secondary action that fails to enter system-menu tracking.
// Drive the full demo view with real pointer events and sample material pixels
// outside child content. No native window is opened. GPU/native acceptance is
// separate from this software-renderer check.
func scenarioLiquidGlass(t *testing.T) {
	type result struct {
		Theme, Surface                        string
		HoverPixels, ResetPixels, PressPixels int
		NativeMenu                            bool
	}
	var results []result
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(results, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "liquid-glass-feedback.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, surface := range []string{"view-switch", "search", "secondary-action"} {
			t.Run(theme+"/"+surface, func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", "macos")
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				tt.SetScale(2)
				role, name := ui.RoleRadioGroup, "显示方式"
				if surface == "search" {
					role, name = ui.RoleSearch, ""
				}
				if surface == "secondary-action" {
					role, name = ui.RoleMenuButton, "发送到…"
				}
				info, ok := tt.AccessByRole(role, name)
				if !ok {
					t.Fatalf("glass surface missing: %s", surface)
				}
				rc := info.Bounds
				patch := func(outside bool) []color.RGBA {
					im := tt.Image()
					var pixels []color.RGBA
					if outside {
						for y := int(2*(rc.Y+rc.H/2)) - 8; y < int(2*(rc.Y+rc.H/2))+8; y++ {
							for x := int(2*rc.X) - 2; x < int(2*rc.X); x++ {
								pixels = append(pixels, im.RGBAAt(x, y))
							}
						}
					} else {
						for y := int(2*rc.Y) + 2; y < int(2*rc.Y)+6; y++ {
							for x := int(2 * rc.X); x < int(2*(rc.X+rc.W)); x++ {
								pixels = append(pixels, im.RGBAAt(x, y))
							}
						}
					}
					return pixels
				}
				diff := func(a, b []color.RGBA) int {
					n := 0
					for i := range a {
						if a[i] != b[i] {
							n++
						}
					}
					return n
				}
				base := patch(false)
				x, y := rc.X+rc.W/2, rc.Y+rc.H/2
				tt.Move(x, y)
				tt.Advance(200 * time.Millisecond)
				r := result{Theme: theme, Surface: surface, HoverPixels: diff(base, patch(false))}
				if r.HoverPixels == 0 {
					t.Error("glass rim did not react to hover")
				}
				tt.Move(0, 0)
				tt.Advance(200 * time.Millisecond)
				r.ResetPixels = diff(base, patch(false))
				if r.ResetPixels != 0 {
					t.Errorf("glass rim did not reset after pointer exit: %d pixels", r.ResetPixels)
				}
				if surface == "secondary-action" {
					tt.Move(x, y)
					tt.Advance(200 * time.Millisecond)
					before := patch(true)
					tt.Press(x, y)
					tt.Advance(200 * time.Millisecond)
					r.PressPixels = diff(before, patch(true))
					r.NativeMenu = len(tt.Menu()) > 0
					if !r.NativeMenu {
						t.Error("glass action did not enter system-menu tracking")
					}
					tt.CloseMenu()
					tt.Release(x, y)
				}
				results = append(results, r)
			})
		}
	}
}
