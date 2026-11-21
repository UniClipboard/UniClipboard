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

// Failure cases: entry/rest center drift, missing or nonuniform intermediate
// scale, instant removal on close, retained invisible modal after exit, motion
// under Reduce Motion, or inline tag edits that open a Sheet/mutate on cancel.
// Source difference: fixed BoardUI Settings Modal ec5c5eb3 centers its panel
// and enters from scale .85 with opacity/blur over 300ms. The superseded macOS
// top-anchor/negative-Y slide is not this source's geometry. Preserve the live
// exit, cancellation, pixel-difference and inline-tag behavior assertions.
func scenarioSheetMotion(t *testing.T) {
	t.Run("resize", scenarioSheetResizeMotion)
	type receipt struct {
		Theme, Sheet, Close      string
		Presentation             string
		EnterY, RestY            float32
		EnterBounds, RestBounds  ui.Rect
		EntryScaleX, EntryScaleY float32
		ExitPixels               int
		ReducedMotion            bool
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
			for _, sheet := range []string{"管理标签", "添加设备", "添加标签"} {
				for _, closeBy := range []string{"done", "escape", "backdrop"} {
					t.Run(mode+"/"+theme+"/"+sheet+"/"+closeBy, func(t *testing.T) {

						d := newDemo("history", theme)
						tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark", Preferences: ui.Preferences{ReduceMotion: reduced}})
						proof.Capture(t, tt)
						label := sheet
						if sheet == "添加设备" {
							if err := tt.Click("管理设备…"); err != nil {
								t.Fatal(err)
							}
						}
						if err := tt.Click(label); err != nil {
							t.Fatal(err)
						}
						tt.Advance(40 * time.Millisecond)
						if sheet == "添加标签" {
							if _, ok := tt.AccessByRole(ui.RoleDialog, sheet); ok {
								t.Fatal("inline tag assignment opened a Sheet")
							}
							editor, ok := tt.AccessByRole(ui.RoleComboBox, "标签名")
							if !ok {
								t.Fatal("missing inline tag editor")
							}
							originalTags := slices.Clone(d.current().tags)
							const name = "motion-inline-tag"
							tt.Type(name)
							switch closeBy {
							case "done":
								tt.Key(0, ui.KeyEnter)
							case "escape":
								tt.Key(0, ui.KeyEscape)
							case "backdrop":
								tt.ClickAt(1090, 710)
							}
							tt.Advance(350 * time.Millisecond)
							if _, ok := tt.AccessByRole(ui.RoleComboBox, "标签名"); ok {
								t.Fatal("inline tag editor retained after dismissal")
							}
							if _, ok := tt.AccessByRole(ui.RoleDialog, sheet); ok {
								t.Fatal("inline tag dismissal left a modal")
							}
							confirmed := closeBy == "done"
							if (d.tagByID(name) != nil) != confirmed || slices.Contains(d.current().tags, name) != confirmed {
								t.Fatal("inline tag confirmation/cancellation changed the wrong state")
							}
							if !confirmed && !slices.Equal(originalTags, d.current().tags) {
								t.Fatal("inline tag cancellation changed existing assignments")
							}
							results = append(results, receipt{Theme: theme, Sheet: sheet, Close: closeBy, Presentation: "inline", ReducedMotion: reduced, EnterY: editor.Bounds.Y})
							if err := tt.Click("管理标签"); err != nil {
								t.Fatalf("inline editor blocked input after dismissal: %v", err)
							}
							return
						}
						entering, ok := tt.AccessByRole(ui.RoleDialog, sheet)
						if !ok {
							t.Fatal("missing sheet")
						}
						r := receipt{Theme: theme, Sheet: sheet, Close: closeBy, Presentation: "sheet", ReducedMotion: reduced, EnterY: entering.Bounds.Y, EnterBounds: entering.Bounds}
						results = append(results, r)
						at := len(results) - 1
						tt.Advance(350 * time.Millisecond)
						resting, ok := tt.AccessByRole(ui.RoleDialog, sheet)
						if !ok {
							t.Fatal("sheet disappeared before reaching its resting state")
						}
						results[at].RestY, results[at].RestBounds = resting.Bounds.Y, resting.Bounds
						centered := func(phase string, bounds ui.Rect) {
							t.Helper()
							if math.Abs(float64(bounds.X+bounds.W/2-550)) > 1 || math.Abs(float64(bounds.Y+bounds.H/2-360)) > 1 {
								t.Fatalf("sheet %s drifted from the viewport center: %+v", phase, bounds)
							}
						}
						centered("entry", entering.Bounds)
						centered("rest", resting.Bounds)
						if resting.Bounds.W <= 0 || resting.Bounds.H <= 0 {
							t.Fatalf("sheet has empty resting geometry: %+v", resting.Bounds)
						}
						xScale, yScale := entering.Bounds.W/resting.Bounds.W, entering.Bounds.H/resting.Bounds.H
						results[at].EntryScaleX, results[at].EntryScaleY = xScale, yScale
						if reduced {
							for _, pair := range [][2]float32{{entering.Bounds.X, resting.Bounds.X}, {entering.Bounds.Y, resting.Bounds.Y}, {entering.Bounds.W, resting.Bounds.W}, {entering.Bounds.H, resting.Bounds.H}} {
								if math.Abs(float64(pair[0]-pair[1])) > .5 {
									t.Fatalf("sheet animated geometry under Reduce Motion: entry=%+v rest=%+v", entering.Bounds, resting.Bounds)
								}
							}
						} else if xScale <= .85 || xScale >= 1 || yScale <= .85 || yScale >= 1 || math.Abs(float64(xScale-yScale)) > .01 {
							t.Fatalf("sheet did not enter through source-centered uniform scale .85 to 1: entry=%+v rest=%+v scale=(%g,%g)", entering.Bounds, resting.Bounds, xScale, yScale)
						}
						switch closeBy {
						case "done":
							closeBtn := "完成"
							if sheet == "添加设备" {
								closeBtn = "关闭"
							}
							if err := tt.Click(closeBtn); err != nil {
								t.Fatal(err)
							}
						case "escape":
							tt.Key(0, ui.KeyEscape)
						case "backdrop":
							tt.ClickAt(20, 650)
						}
						tt.Advance(60 * time.Millisecond)
						_, liveDuringExit := tt.AccessByRole(ui.RoleDialog, sheet)
						if !reduced && !liveDuringExit {
							t.Fatal("sheet removed its live modal tree before the source exit completed")
						}
						if reduced && liveDuringExit {
							t.Fatal("sheet retained an animated exit under Reduce Motion")
						}
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
						afterBtn := "管理标签"
						if sheet == "添加设备" {
							afterBtn = "返回历史"
						}
						if err := tt.Click(afterBtn); err != nil {
							t.Fatalf("modal blocked input after exit: %v", err)
						}
					})
				}
			}
		}
	}
}
