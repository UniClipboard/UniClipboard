package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"slices"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: the live table is replaced and relaid out during exit;
// selection clears early; or an opaque sheet surface changes brightness.
func scenarioSheetExitStability(t *testing.T) {
	type sample struct {
		Theme, Close string
		Milliseconds int
		Y            float32
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
				t.Setenv("UNIC_UI_SKIN", "macos")
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if err := tt.Click("管理标签"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(350 * time.Millisecond)
				if err := tt.Click("#设计"); err != nil {
					t.Fatal(err)
				}
				read := func(ms int) sample {
					dialog, ok := tt.AccessByRole(ui.RoleDialog, "管理")
					if !ok {
						t.Fatal("live sheet removed before exit completed")
					}
					r := sample{Theme: theme, Close: closeBy, Milliseconds: ms, Y: dialog.Bounds.Y, Selection: slices.Clone(d.tagManager.table.Selection())}
					for _, name := range []string{"名称", "条目", "侧边栏"} {
						col, ok := tt.AccessByRole(ui.RoleColumnHeader, name)
						if !ok {
							t.Fatal("table column removed during exit")
						}
						r.Widths = append(r.Widths, col.Bounds.W)
					}
					im := tt.Image()
					scale := float32(im.Bounds().Dx()) / 1100
					// Sample the opaque left sheet gutter, away from controls and shadows.
					v := im.RGBAAt(int((dialog.Bounds.X+8)*scale), int((dialog.Bounds.Y+150)*scale))
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
				for _, ms := range []int{16, 32, 48} {
					tt.Advance(16 * time.Millisecond)
					after := read(ms)
					if !slices.Equal(before.Widths, after.Widths) {
						t.Fatalf("table widths changed: %v -> %v", before.Widths, after.Widths)
					}
					if !slices.Equal(before.Selection, after.Selection) {
						t.Fatalf("selection cleared during exit: %v -> %v", before.Selection, after.Selection)
					}
					if before.Surface != after.Surface {
						t.Fatalf("sheet surface brightness changed: %v -> %v", before.Surface, after.Surface)
					}
				}
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleDialog, "管理"); ok {
					t.Fatal("sheet still present after exit")
				}
				if len(d.tagManager.table.Selection()) != 0 {
					t.Fatal("selection not cleaned after exit")
				}
			})
		}
	}
}
