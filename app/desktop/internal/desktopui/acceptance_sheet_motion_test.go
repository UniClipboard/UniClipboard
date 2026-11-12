package desktopui

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: no intermediate entry position, instant removal on close,
// a retained invisible modal after exit, or motion under Reduce Motion.
func scenarioSheetMotion(t *testing.T) {
	t.Run("resize", scenarioSheetResizeMotion)
	type receipt struct {
		Theme, Sheet, Close string
		EnterY, RestY       float32
		ExitPixels          int
		ReducedMotion       bool
	}
	var results []receipt
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
			if err := os.WriteFile(filepath.Join(dir, "sheet-motion.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, reduced := range []bool{false, true} {
		mode := "motion"
		if reduced {
			mode = "reduced"
		}
		for _, theme := range []string{"dark", "light"} {
			for _, sheet := range []string{"管理", "管理设备", "添加标签"} {
				for _, closeBy := range []string{"done", "escape", "backdrop"} {
					t.Run(mode+"/"+theme+"/"+sheet+"/"+closeBy, func(t *testing.T) {
						t.Setenv("UNIC_UI_SKIN", "macos")
						d := newDemo("history", theme)
						tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark", Preferences: ui.Preferences{ReduceMotion: reduced}})
						proof.Capture(t, tt)
						label := sheet
						if sheet == "管理设备" {
							label = "管理设备…"
						}
						if sheet == "管理" {
							label = "管理标签"
						}
						if err := tt.Click(label); err != nil {
							t.Fatal(err)
						}
						tt.Advance(40 * time.Millisecond)
						entering, ok := tt.AccessByRole(ui.RoleDialog, sheet)
						if !ok {
							t.Fatal("missing sheet")
						}
						r := receipt{Theme: theme, Sheet: sheet, Close: closeBy, ReducedMotion: reduced, EnterY: entering.Bounds.Y}
						results = append(results, r)
						at := len(results) - 1
						if reduced && entering.Bounds.Y != 0 {
							t.Fatalf("sheet moved under Reduce Motion: %+v", entering.Bounds)
						}
						if !reduced && (entering.Bounds.Y >= 0 || entering.Bounds.Y <= -entering.Bounds.H) {
							t.Fatalf("no intermediate sheet entry: %+v", entering.Bounds)
						}
						tt.Advance(350 * time.Millisecond)
						resting, _ := tt.AccessByRole(ui.RoleDialog, sheet)
						results[at].RestY = resting.Bounds.Y
						if resting.Bounds.Y != 0 {
							t.Fatalf("sheet did not settle at top: %+v", resting.Bounds)
						}
						switch closeBy {
						case "done":
							if err := tt.Click("完成"); err != nil {
								t.Fatal(err)
							}
						case "escape":
							tt.Key(0, ui.KeyEscape)
						case "backdrop":
							tt.ClickAt(20, 650)
						}
						tt.Advance(60 * time.Millisecond)
						im := tt.Image()
						scale := im.Bounds().Dx() / 1100
						samples := make([][4]uint8, 0)
						for y := 20; y < 140; y += 3 {
							for x := 260; x < 820; x += 3 {
								v := im.RGBAAt(x*scale, y*scale)
								samples = append(samples, [4]uint8{v.R, v.G, v.B, v.A})
							}
						}
						tt.Advance(350 * time.Millisecond)
						im = tt.Image()
						j, diff := 0, 0
						for y := 20; y < 140; y += 3 {
							for x := 260; x < 820; x += 3 {
								v := im.RGBAAt(x*scale, y*scale)
								if samples[j] != [4]uint8{v.R, v.G, v.B, v.A} {
									diff++
								}
								j++
							}
						}
						results[at].ExitPixels = diff
						if !reduced && diff == 0 {
							t.Fatal("sheet disappeared without an exit animation")
						}
						if _, ok := tt.AccessByRole(ui.RoleDialog, sheet); ok {
							t.Fatal("sheet retained after exit")
						}
						if err := tt.Click("管理标签"); err != nil {
							t.Fatalf("modal blocked input after exit: %v", err)
						}
					})
				}
			}
		}
	}
}
