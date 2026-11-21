package desktopui

import (
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Source-change protocol: the old opaque top-slide is replaced by BoardUI
// center-scale/fade/blur. Visual widths must scale; logical column widths and
// selection must stay stable until the live exit scope is removed. Preserve
// raster samples as fade evidence, rather than requiring an opaque surface.
func scenarioSheetExitStability(t *testing.T) {
	type sample struct {
		Theme, Close string
		Milliseconds int
		Y            float32
		Panel        ui.Rect
		Scale        float32
		Widths       []float32
		Surface      [3]uint8
		Selection    []string
	}
	var results []sample
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
			if err := os.WriteFile(filepath.Join(dir, "sheet-exit-stability.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, closeBy := range []string{"done", "escape", "backdrop"} {
			t.Run(theme+"/"+closeBy, func(t *testing.T) {

				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				if err := boarduiTableClick(tt, "标签", "#设计", 0, false); err != nil {
					t.Fatal(err)
				}
				rest, ok := tt.AccessByRole(ui.RoleDialog, "管理标签")
				if !ok {
					t.Fatal("resting sheet missing")
				}
				read := func(ms int) sample {
					dialog, ok := tt.AccessByRole(ui.RoleDialog, "管理标签")
					if !ok {
						t.Fatal("live sheet removed before exit completed")
					}
					r := sample{Theme: theme, Close: closeBy, Milliseconds: ms, Y: dialog.Bounds.Y, Panel: dialog.Bounds, Scale: dialog.Bounds.W / rest.Bounds.W, Selection: slices.Clone(d.tagManager.selected)}
					if list, ok := tt.AccessByRole(ui.RoleList, "标签"); ok {
						r.Widths = append(r.Widths, list.Bounds.W/r.Scale)
					} else {
						t.Fatal("tag list removed during exit")
					}
					im := tt.Image()
					scale := float32(im.Bounds().Dx()) / 1100
					// Sample the scaled left panel gutter, away from controls and shadows.
					v := im.RGBAAt(int((dialog.Bounds.X+8*r.Scale)*scale), int((dialog.Bounds.Y+150*r.Scale)*scale))
					r.Surface = [3]uint8{v.R, v.G, v.B}
					results = append(results, r)
					return r
				}
				before := read(0)
				switch closeBy {
				case "done":
					if err := tt.Click("完成"); err != nil {
						t.Fatal(err)
					}
				case "escape":
					tt.Key(0, ui.KeyEscape)
				case "backdrop":
					tt.ClickAt(20, 500)
				}
				faded := false
				for _, ms := range []int{16, 32, 48} {
					tt.Advance(16 * time.Millisecond)
					after := read(ms)
					if len(before.Widths) != len(after.Widths) {
						t.Fatal("list width count changed")
					}
					for i, w := range before.Widths {
						if math.Abs(float64(w-after.Widths[i])) > .5 {
							t.Fatalf("logical widths changed during source scale: %v -> %v", before.Widths, after.Widths)
						}
					}
					if after.Scale > 1 || after.Scale < .85 {
						t.Fatalf("source exit scale out of range: %g", after.Scale)
					}
					if !slices.Equal(before.Selection, after.Selection) {
						t.Fatalf("selection cleared during exit: %v -> %v", before.Selection, after.Selection)
					}
					faded = faded || before.Surface != after.Surface
				}
				if !faded {
					t.Fatal("source fade produced no raster change at panel gutter")
				}
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleDialog, "管理标签"); ok {
					t.Fatal("sheet still present after exit")
				}
				if len(d.tagManager.selected) != 0 {
					t.Fatal("selection not cleaned after exit")
				}
			})
		}
	}
}
