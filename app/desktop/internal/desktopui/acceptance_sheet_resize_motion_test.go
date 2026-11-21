package desktopui

import (
	"encoding/json"
	"fmt"
	"image/png"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// Failure inventory: instant width/height changes, center drift, stretched
// content, discontinuous reversal, premature modal removal, retained input
// after exit, and spatial motion under Reduce Motion.
// Source difference: the fixed BoardUI Settings Modal ec5c5eb3 is centered,
// not top-anchored. Dynamic application content must resize around that same
// center; retain intermediate-size, reversal, dismissal and reopen assertions.
func scenarioSheetResizeMotion(t *testing.T) {
	type sample struct {
		Theme   string
		Reduced bool
		Step    string
		Bounds  ui.Rect
	}
	var samples []sample
	dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	t.Cleanup(func() {
		b, err := json.MarshalIndent(samples, "", "  ")
		if err == nil {
			err = os.WriteFile(filepath.Join(dir, "sheet-resize-motion.json"), append(b, '\n'), 0644)
		}
		if err != nil {
			t.Error(err)
		}
	})
	for _, theme := range []string{"dark", "light"} {
		for _, reduced := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/reduced-%t", theme, reduced), func(t *testing.T) {

				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark", Preferences: ui.Preferences{ReduceMotion: reduced}})
				click := func(label string) {
					t.Helper()
					if err := tt.Click(label); err != nil {
						t.Fatal(err)
					}
				}
				invite := func() {
					t.Helper()
					click("添加设备")
				}
				read := func(step string) ui.Rect {
					t.Helper()
					n, ok := tt.AccessByRole(ui.RoleDialog, "")
					if !ok {
						t.Fatal("missing live sheet")
					}
					r := n.Bounds
					if math.Abs(float64(r.X+r.W/2-550)) > 1 || math.Abs(float64(r.Y+r.H/2-360)) > 1 {
						t.Fatalf("sheet resize lost the viewport center: %+v", r)
					}
					samples = append(samples, sample{theme, reduced, step, r})
					f, err := os.Create(filepath.Join(dir, fmt.Sprintf("resize-%s-%t-%s.png", theme, reduced, step)))
					if err != nil {
						t.Fatal(err)
					}
					err = png.Encode(f, tt.Image())
					closeErr := f.Close()
					if err != nil {
						t.Fatal(err)
					}
					if closeErr != nil {
						t.Fatal(closeErr)
					}
					return r
				}
				between := func(a, mid, b ui.Rect) {
					t.Helper()
					if reduced {
						if mid != b {
							t.Fatalf("resize moved under Reduce Motion: %+v -> %+v", mid, b)
						}
						return
					}
					for i, v := range [][3]float32{{a.W, mid.W, b.W}, {a.H, mid.H, b.H}} {
						if v[0] == v[2] {
							continue
						}
						if v[1] <= min(v[0], v[2]) || v[1] >= max(v[0], v[2]) {
							t.Fatalf("no intermediate size on axis %d: %v", i, v)
						}
					}
				}
				click("管理设备…")
				invite()
				tt.Advance(350 * time.Millisecond)
				compact := read("compact")
				click("跨网或命令行配对？改用完整票据 →")
				tt.Advance(64 * time.Millisecond)
				heightMid := read("height-changing")
				tt.Advance(350 * time.Millisecond)
				ticket := read("ticket")
				if ticket.H == compact.H {
					t.Fatal("content change did not change height")
				}
				between(compact, heightMid, ticket)
				click("← 返回 6 位配对码")
				tt.Advance(64 * time.Millisecond)
				shrinking := read("shrinking")
				between(ticket, shrinking, compact)
				// Reverse an unfinished height transition.
				click("跨网或命令行配对？改用完整票据 →")
				reversalStart := read("reversal-start")
				tt.Advance(64 * time.Millisecond)
				reversed := read("reversed")
				if !reduced && reversed.H == reversalStart.H {
					t.Fatalf("reversal stalled: %+v -> %+v", reversalStart, reversed)
				}
				tt.Advance(350 * time.Millisecond)
				read("reversed-settled")
				click("← 返回 6 位配对码")
				tt.Advance(350 * time.Millisecond)
				if final := read("compact-restored"); final != compact {
					t.Fatalf("sheet did not restore geometry: %+v -> %+v", compact, final)
				}
				click("跨网或命令行配对？改用完整票据 →")
				tt.Advance(32 * time.Millisecond)
				tt.Key(0, ui.KeyEscape)
				tt.Advance(350 * time.Millisecond)
				if _, ok := tt.AccessByRole(ui.RoleDialog, ""); ok {
					t.Fatal("sheet retained after dismissal during resize")
				}
				invite()
				tt.Advance(350 * time.Millisecond)
				if final := read("reopened"); final != compact {
					t.Fatal("sheet task leaked across close/reopen")
				}
				tt.Key(0, ui.KeyEscape)
				tt.Advance(350 * time.Millisecond)
				tt.Key(0, ui.KeyEscape)
				if d.page != "history" {
					t.Fatalf("devices page did not return to history on Escape: %s", d.page)
				}
			})
		}
	}
}
