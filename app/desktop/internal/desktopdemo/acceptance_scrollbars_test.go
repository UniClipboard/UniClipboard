package desktopdemo

import (
	"encoding/json"
	"fmt"
	"image/color"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure cases: raw detail/preview scrollers showing on hover; an indicator
// never appearing during movement; idle hover keeping it visible; nested
// scrollers sharing activity; a skin bypassing the shared policy. Drive the
// complete fixture view and its real scroll input. Inspect indicator pixels in
// the reserved edge, without opening a native window or exporting screenshots.
func scenarioUnifiedScrollbars(t *testing.T) {
	type result struct {
		Skin, Theme, Area                 string
		Hover, Moving, Idle, ParentMoving int
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
			if err := os.WriteFile(filepath.Join(dir, "scrollbar-policy.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"dark", "light"} {
			for _, area := range []string{"history", "grid", "detail", "preview"} {
				t.Run(skin+"/"+theme+"/"+area, func(t *testing.T) {
					t.Setenv("UNIC_UI_SKIN", skin)
					d := newDemo("history", theme)
					if area == "grid" {
						d.layout = "grid"
					}
					d.entries[0].text = strings.Repeat("长文本预览，用于验证独立滚动。\n", 80)
					for i := 0; i < 24; i++ {
						d.devices = append(d.devices, device{id: fmt.Sprintf("overflow-%d", i), name: fmt.Sprintf("演示设备 %d", i)})
					}
					tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
					proof.Capture(t, tt)
					var box, parentBox ui.Rect
					if area == "history" || area == "grid" {
						label := "历史条目"
						if area == "grid" {
							label = "历史网格"
						}
						list, ok := tt.AccessByRole(ui.RoleList, label)
						if !ok {
							t.Fatal("history list missing")
						}
						box = list.Bounds
					} else {
						detail, ok := tt.AccessByRole(ui.RoleRegion, "历史详情")
						if !ok {
							t.Fatal("detail pane missing")
						}
						var scrolls []ui.Rect
						for _, a := range tt.AccessAll() {
							if a.Role == ui.RoleScroll && a.Bounds.X >= detail.Bounds.X && a.Bounds.W > 100 && a.Bounds.H > 100 {
								scrolls = append(scrolls, a.Bounds)
							}
						}
						slices.SortFunc(scrolls, func(a, b ui.Rect) int {
							if a.H > b.H {
								return -1
							}
							if a.H < b.H {
								return 1
							}
							return 0
						})
						if len(scrolls) < 2 {
							t.Fatalf("nested detail scrollers missing: %v", scrolls)
						}
						box = scrolls[0]
						if area == "preview" {
							parentBox = scrolls[0]
							box = scrolls[1]
						}
					}
					edge := func(box ui.Rect) []color.RGBA {
						im := tt.Image()
						var pixels []color.RGBA
						for y := int(box.Y) + 12; y < int(box.Y+box.H)-12; y++ {
							for x := int(box.X+box.W) - 10; x < int(box.X+box.W)-1; x++ {
								pixels = append(pixels, im.RGBAAt(x, y))
							}
						}
						return pixels
					}
					base := edge(box)
					parentBase := edge(parentBox)
					difference := func(box ui.Rect, base []color.RGBA) int {
						n := 0
						for i, v := range edge(box) {
							if v != base[i] {
								n++
							}
						}
						return n
					}
					x, y := box.X+box.W-14, box.Y+box.H/2
					tt.Move(x, y)
					r := result{Skin: skin, Theme: theme, Area: area, Hover: difference(box, base)}
					tt.Scroll(x, y, 0, 120)
					tt.Advance(16 * time.Millisecond)
					r.Moving = difference(box, base)
					if area == "preview" {
						r.ParentMoving = difference(parentBox, parentBase)
					}
					tt.Advance(800 * time.Millisecond)
					r.Idle = difference(box, base)
					results = append(results, r)
					if r.Hover != 0 {
						t.Errorf("hover displayed the scrollbar: %d edge pixels", r.Hover)
					}
					if r.Moving == 0 {
						t.Error("scrolling did not display the scrollbar")
					}
					if r.Idle != 0 {
						t.Errorf("idle scrollbar remained visible: %d edge pixels", r.Idle)
					}
					if r.ParentMoving != 0 {
						t.Errorf("scrolling the preview displayed the idle parent scrollbar: %d edge pixels", r.ParentMoving)
					}
				})
			}
		}
	}
}
