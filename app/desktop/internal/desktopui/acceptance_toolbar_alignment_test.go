package desktopui

import (
	"encoding/json"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure inventory: toolbar shrinks under body pressure; asymmetric vertical
// insets; expanded search pushes commands outside compact windows; menu hit
// targets lose alignment. Drive the complete history view at supported sizes.
func scenarioToolbarAlignment(t *testing.T) {

	results := map[string]map[string]ui.Rect{}
	rimResults := map[string]map[string]float64{}
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(map[string]any{"geometry": results, "visible_control_pixels": rimResults}, "", "  ")
			if err != nil {
				t.Error(err)
				return
			}
			if err := os.WriteFile(filepath.Join(dir, "toolbar-alignment.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, width := range []int{600, 800, 1100, 1440} {
		height := 480
		if width == 1100 {
			height = 720
		}
		scale := float32(1)
		if width == 600 || width == 1100 {
			scale = 2
		}
		for _, layout := range []string{"list", "grid"} {
			for _, expanded := range []bool{false, true} {
				for _, theme := range []string{"dark", "light"} {
					name := fmt.Sprintf("%s/%dx%d/scale%.0f/%s/search%t", theme, width, height, scale, layout, expanded)
					t.Run(name, func(t *testing.T) {
						d := newDemo("history", theme)
						d.layout = layout
						tt := ui.NewTesterWith(d.view, width, height, ui.TesterOptions{Dark: theme == "dark"})
						tt.SetScale(scale)
						if expanded {
							tt.Key(ui.Cmd, ui.KeyF)
							tt.Advance(350 * time.Millisecond)
						}
						proof.Capture(t, tt)
						pin, ok := tt.AccessByRole(ui.RoleButton, "切换置顶")
						if !ok {
							t.Fatal("pin missing")
						}
						results[name] = map[string]ui.Rect{"pin": pin.Bounds}
						bar, ok := tt.AccessByRole(ui.RoleToolbar, "全部")
						if !ok {
							t.Fatal("toolbar missing")
						}
						results[name]["toolbar"] = bar.Bounds
						if theme == "light" {
							// Retain a pixel visibility assertion for every command, but
							// sample the actual BoardUI controls. A removed glass capsule
							// outline is not part of the unified source reference.
							frames := map[string]ui.Rect{}
							for _, target := range []struct {
								role ui.Role
								name string
							}{
								{ui.RoleGroup, "显示方式"}, {ui.RoleSearch, ""},
								{ui.RoleMenuButton, "发送到设备"}, {ui.RoleMenuButton, "更多操作"},
							} {
								info, ok := tt.AccessByRole(target.role, target.name)
								if !ok {
									t.Fatalf("toolbar control missing: %v %q", target.role, target.name)
								}
								frames[string(target.role)+":"+target.name] = info.Bounds
							}
							rimResults[name] = map[string]float64{}
							im := tt.Image()
							for label, frame := range frames {
								background := im.RGBAAt(int((frame.X+frame.W/2)*scale), int((frame.Y-5)*scale))
								visible := 0
								for y := int(frame.Y * scale); y < int((frame.Y+frame.H)*scale); y++ {
									for x := int(frame.X * scale); x < int((frame.X+frame.W)*scale); x++ {
										ink := im.RGBAAt(x, y)
										delta := absBoardUIChannel(int(ink.R)-int(background.R)) + absBoardUIChannel(int(ink.G)-int(background.G)) + absBoardUIChannel(int(ink.B)-int(background.B))
										if delta >= 36 {
											visible++
										}
									}
								}
								rimResults[name][label] = float64(visible)
								if visible < 12 {
									t.Errorf("light BoardUI control is not visible: %s pixels=%d", label, visible)
								}
							}
						}

						if expanded {
							search, ok := tt.AccessByRole(ui.RoleSearch, "")
							if !ok {
								t.Fatal("search missing")
							}
							input, ok := tt.AccessByRole(ui.RoleComboBox, "搜索历史")
							if !ok {
								t.Fatal("search input missing")
							}
							b, frame := input.Bounds, search.Bounds
							if b.X < frame.X || b.X+b.W > frame.X+frame.W || b.Y < frame.Y || b.Y+b.H > frame.Y+frame.H {
								t.Errorf("search hit target outside visible field: input=%+v field=%+v", b, frame)
							}
						}

						if math.Abs(float64(bar.Bounds.H-52)) > .1 {
							t.Errorf("toolbar height=%v, want 52", bar.Bounds.H)
						}
						for _, a := range tt.AccessAll() {
							if a.Name != "切换置顶" && a.Name != "发送到设备" && a.Name != "更多操作" && a.Name != "显示方式" && a.Role != ui.RoleSearch {
								continue
							}
							results[name][string(a.Role)+":"+a.Name] = a.Bounds
							b := a.Bounds
							if b.X < bar.Bounds.X || b.X+b.W > bar.Bounds.X+bar.Bounds.W+.1 || b.Y < bar.Bounds.Y+8 || b.Y+b.H > bar.Bounds.Y+bar.Bounds.H-8+.1 {
								t.Errorf("%s outside toolbar insets: control=%+v toolbar=%+v", a.Name, b, bar.Bounds)
							}
						}
						for _, label := range []string{"发送到设备", "更多操作"} {
							a, ok := tt.AccessByRole(ui.RoleMenuButton, label)
							if !ok {
								t.Fatalf("trigger missing: %s", label)
							}
							results[name][label] = a.Bounds
							if math.Abs(float64(a.Bounds.Y+a.Bounds.H/2-pin.Bounds.Y-pin.Bounds.H/2)) > .1 {
								t.Errorf("%s is not centered with pin: menu=%+v pin=%+v", label, a.Bounds, pin.Bounds)
							}
						}
					})
				}
			}
		}
	}
}
