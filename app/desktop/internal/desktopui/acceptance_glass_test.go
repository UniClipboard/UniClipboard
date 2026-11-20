package desktopui

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

// Migrates the former liquid-glass acceptance to the single BoardUI visual
// language. Preserve real hover/reset, action visibility and menu interaction.
// Reactive glass rims and system-menu presentation were platform-skin behavior;
// they are intentionally replaced by source-colored surfaces and real popups.
func scenarioBoardUISurfaces(t *testing.T) {
	type result struct {
		Theme, Surface           string
		HoverPixels, ResetPixels int
		PopupItems               []string
		VisibleAction            bool
	}
	var results []result
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			b, e := json.MarshalIndent(results, "", "  ")
			if e != nil {
				t.Error(e)
				return
			}
			if e = os.WriteFile(filepath.Join(dir, "boardui-surfaces.json"), append(b, '\n'), 0644); e != nil {
				t.Error(e)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		for _, surface := range []string{"view-switch", "search", "secondary-action"} {
			t.Run(theme+"/"+surface, func(t *testing.T) {
				d := newDemo("history", theme)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				tt.SetScale(2)
				proof.Capture(t, tt)
				role, name := ui.RoleGroup, "显示方式"
				if surface == "search" {
					role, name = ui.RoleSearch, ""
				}
				if surface == "secondary-action" {
					role, name = ui.RoleMenuButton, "发送到…"
				}
				info, ok := tt.AccessByRole(role, name)
				if !ok {
					t.Fatal("BoardUI surface missing:", surface)
				}
				rc := info.Bounds
				patch := func() []color.RGBA {
					im := tt.Image()
					var pixels []color.RGBA
					for y := int(rc.Y * 2); y < int((rc.Y+rc.H)*2); y++ {
						for x := int(rc.X * 2); x < int((rc.X+rc.W)*2); x++ {
							pixels = append(pixels, im.RGBAAt(x, y))
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
				base := patch()
				x, y := rc.X+rc.W/2, rc.Y+rc.H/2
				if surface == "view-switch" {
					target, ok := tt.AccessByRole(ui.RoleToggleButton, "网格")
					if !ok {
						t.Fatal("unselected BoardUI toggle missing")
					}
					x, y = target.Bounds.X+target.Bounds.W/2, target.Bounds.Y+target.Bounds.H/2
				}
				tt.Move(x, y)
				tt.Advance(500 * time.Millisecond)
				r := result{Theme: theme, Surface: surface, HoverPixels: diff(base, patch())}
				if r.HoverPixels == 0 {
					t.Error("BoardUI surface did not expose hover feedback")
				}
				tt.Move(0, 0)
				tt.Advance(500 * time.Millisecond)
				r.ResetPixels = diff(base, patch())
				if r.ResetPixels != 0 {
					t.Errorf("hover feedback did not reset: %d pixels", r.ResetPixels)
				}
				if surface == "secondary-action" {
					copy, ok := tt.AccessByRole(ui.RoleButton, "复制")
					if !ok || copy.Bounds.W <= 0 || copy.Bounds.H <= 0 {
						t.Fatal("copy action missing")
					}
					// Preserve the former action-visibility pixel check using the flat
					// BoardUI action itself, rather than a removed glass outline.
					image := tt.Image()
					background := image.RGBAAt(int((copy.Bounds.X+copy.Bounds.W/2)*2), int((copy.Bounds.Y-5)*2))
					visiblePixels := 0
					for py := int(copy.Bounds.Y * 2); py < int((copy.Bounds.Y+copy.Bounds.H)*2); py++ {
						for px := int(copy.Bounds.X * 2); px < int((copy.Bounds.X+copy.Bounds.W)*2); px++ {
							ink := image.RGBAAt(px, py)
							delta := absBoardUIChannel(int(ink.R)-int(background.R)) + absBoardUIChannel(int(ink.G)-int(background.G)) + absBoardUIChannel(int(ink.B)-int(background.B))
							if delta >= 36 {
								visiblePixels++
							}
						}
					}
					if visiblePixels < 12 {
						t.Fatal("copy action is not visibly distinct from its background")
					}
					r.VisibleAction = true
					if e := tt.Click("发送到…"); e != nil {
						t.Fatal(e)
					}
					r.PopupItems = boarduiMenuItems(tt)
					if len(r.PopupItems) == 0 {
						t.Fatal("send popup did not open")
					}
					if len(boarduiMenuItems(tt)) == 0 {
						t.Fatal("in-window popup has no accessible items")
					}
					tt.Key(0, ui.KeyEscape)
					if d.notice != "" {
						t.Fatal("canceling send popup performed the action")
					}
				}
				results = append(results, r)
			})
		}
	}
}

func absBoardUIChannel(value int) int {
	if value < 0 {
		return -value
	}
	return value
}
