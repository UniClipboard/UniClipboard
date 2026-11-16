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
	t.Setenv("UNIC_UI_SKIN", "macos")
	results := map[string]map[string]ui.Rect{}
	rimResults := map[string]map[string]float64{}
	defer func() {
		if dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS"); dir != "" {
			if err := os.MkdirAll(dir, 0755); err != nil {
				t.Error(err)
				return
			}
			b, err := json.MarshalIndent(map[string]any{"geometry": results, "rim_contrast": rimResults}, "", "  ")
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
							layout, ok := tt.AccessByRole(ui.RoleRadioGroup, "显示方式")
							if !ok {
								t.Fatal("layout control missing")
							}
							search, ok := tt.AccessByRole(ui.RoleSearch, "")
							if !ok {
								t.Fatal("search missing")
							}
							send, _ := tt.AccessByRole(ui.RoleMenuButton, "发送到设备")
							more, _ := tt.AccessByRole(ui.RoleMenuButton, "更多操作")
							frames := map[string]ui.Rect{"layout": layout.Bounds, "search": search.Bounds, "commands": {X: send.Bounds.X, Y: layout.Bounds.Y, W: more.Bounds.X + more.Bounds.W - send.Bounds.X, H: layout.Bounds.H}}
							rimResults[name] = map[string]float64{}
							im := tt.Image()
							for label, frame := range frames {
								for _, bottom := range []bool{false, true} {

									visible, count := 0, 0
									for x := frame.X + frame.W*.3; x < frame.X+frame.W*.7; x += 1 {
										// Follow the capsule arc, and sample beyond the focus halo.
										radius := frame.H / 2
										dx := max(float32(0), max(frame.X+radius-x, x-(frame.X+frame.W-radius)))
										arc := radius - float32(math.Sqrt(float64(radius*radius-dx*dx)))
										edge, outside := frame.Y+arc, frame.Y+arc-7
										if bottom {
											edge, outside = frame.Y+frame.H-arc, frame.Y+frame.H-arc+7
										}
										bg := im.RGBAAt(int(x*scale), int(outside*scale))
										contrast := 0
										for dy := -int(scale); dy < 2*int(scale); dy++ {
											px := im.RGBAAt(int(x*scale), int(edge*scale)+dy)
											delta := int(bg.R) + int(bg.G) + int(bg.B) - int(px.R) - int(px.G) - int(px.B)
											if delta > contrast {
												contrast = delta
											}
										}
										if contrast >= 36 {
											visible++
										}
										count++
									}
									ratio := float64(visible) / float64(count)
									edge := fmt.Sprintf("%s/bottom%t", label, bottom)
									rimResults[name][edge] = ratio
									if ratio < .8 {
										t.Errorf("light capsule rim is lost against the background: %s visible=%.2f, want >=0.80", edge, ratio)
									}
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
