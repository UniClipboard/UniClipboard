package desktopui

import (
	"bytes"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"math"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

// Failure inventory precedes implementation: API metadata dropped during projection;
// cached icon absent or confused with clipboard content; missing name/icon breaks
// the header; unknown/previous selection retains stale provenance; a long name
// displaces device/time or commands; translated UI renames application identity.
// Exercise API-to-view projection and the complete history composition. No native
// window, browser, system clipboard or external icon lookup is used.
func scenarioSourceApplication(t *testing.T) {
	img := image.NewNRGBA(image.Rect(0, 0, 16, 16))
	for y := 0; y < 16; y++ {
		for x := 0; x < 16; x++ {
			img.SetNRGBA(x, y, color.NRGBA{229, 30, 85, 255})
		}
	}
	var encoded bytes.Buffer
	if err := png.Encode(&encoded, img); err != nil {
		t.Fatal(err)
	}
	application := func(name string, icon []byte) *engine.SourceApplication {
		return &engine.SourceApplication{Platform: "darwin", Identifier: "org.example.Editor", Name: name, Attribution: "source_marker", IconPNG: icon}
	}
	cases := []struct {
		name, label string
		app         *engine.SourceApplication
		icon        bool
	}{
		{"icon", "Example Editor", application("Example Editor", encoded.Bytes()), true},
		{"no-icon", "Example Editor", application("Example Editor", nil), false},
		{"identifier", "org.example.Editor", application("", nil), false},
		{"invalid-icon", "Example Editor", application("Example Editor", []byte("bad PNG")), false},
		{"unknown", "", nil, false},
		{"long", strings.TrimSpace(strings.Repeat("Long Application Name ", 8)), application(strings.TrimSpace(strings.Repeat("Long Application Name ", 8)), encoded.Bytes()), true},
	}
	type measurement struct {
		Skin, Theme, Case               string
		Width                           int
		Application, Icon, Time, Detail ui.Rect
	}
	var results []measurement
	defer func() {
		dir := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
		if dir == "" {
			t.Error("missing artifact directory")
			return
		}
		b, err := json.MarshalIndent(results, "", "  ")
		if err != nil {
			t.Error(err)
			return
		}
		if err = os.WriteFile(filepath.Join(dir, "source-application.json"), b, 0600); err != nil {
			t.Error(err)
		}
	}()
	for _, skin := range []string{"macos", "linux", "windows"} {
		for _, theme := range []string{"dark", "light"} {
			for _, sample := range cases {
				for _, width := range []int{1100, 640} {
					if width == 640 && sample.name != "long" {
						continue
					}
					t.Run(fmt.Sprintf("%s-%s-%s-%d", skin, theme, sample.name, width), func(t *testing.T) {
						t.Setenv("UNIC_UI_SKIN", skin)
						d := newDemo("history", theme)
						d.language = "zh-CN"
						item := api.HistoryItem{ID: "source-record", Kind: "text", Origin: "remote_device", DeviceID: "remote-workstation", CreatedAt: time.Now().UnixMilli(), SourceApplication: sample.app}
						v := projectHistory(item)
						v.title = "Source metadata fixture"
						v.text = "Synthetic clipboard content"
						legacy := projectHistory(api.HistoryItem{ID: "legacy-record", Kind: "text", Origin: "explicit", CreatedAt: item.CreatedAt})
						legacy.title = "No application metadata"
						legacy.text = "Legacy content"
						d.entries = []entry{v, legacy}
						d.devices = append(d.devices, device{id: item.DeviceID, name: "Workstation", online: true})
						d.selectEntry(v.id)
						d.detailOpen = true
						tt := ui.NewTesterWith(d.view, width, 720, ui.TesterOptions{Dark: theme == "dark"})
						proof.Capture(t, tt)
						var normalTimeWidth float32
						if sample.name == "long" {
							brief := item
							brief.SourceApplication = application("Example Editor", encoded.Bytes())
							short := projectHistory(brief)
							short.title = v.title
							short.text = v.text
							d.entries[0] = short
							tt.Frame()
							timeText := d.sourceName(v.source) + " · " + d.displayDate(v.date)
							normal, _ := tt.AccessByRole(ui.RoleText, timeText)
							normalTimeWidth = normal.Bounds.W
							d.entries[0] = v
							tt.Frame()
						}
						group, exists := tt.AccessByRole(ui.RoleGroup, "来源应用")
						if sample.app == nil {
							if exists {
								t.Fatal("unknown source must not show an application")
							}
							return
						}
						if !exists {
							t.Fatal("source application is missing from the history detail header")
						}
						name, ok := tt.AccessByRole(ui.RoleText, sample.label)
						if !ok {
							t.Fatal("application display name or identifier fallback is missing")
						}
						icon, ok := tt.AccessByRole(ui.RoleImage, sample.label)
						if !ok {
							t.Fatal("source icon or generic fallback is missing")
						}
						stamp, ok := tt.AccessByRole(ui.RoleText, d.sourceName(v.source)+" · "+d.displayDate(v.date))
						if !ok {
							t.Fatal("device/time missing")
						}
						if normalTimeWidth > 0 && stamp.Bounds.W < normalTimeWidth-1 {
							t.Fatal("long application name displaces device/time")
						}
						detail, ok := tt.AccessByRole(ui.RoleRegion, "历史详情")
						if !ok {
							t.Fatal("history detail missing")
						}
						results = append(results, measurement{skin, theme, sample.name, width, group.Bounds, icon.Bounds, stamp.Bounds, detail.Bounds})
						if group.Bounds.W <= 16 || name.Bounds.W <= 0 || icon.Bounds.W != 16 || icon.Bounds.H != 16 {
							t.Fatalf("invalid application geometry: group=%+v name=%+v icon=%+v", group.Bounds, name.Bounds, icon.Bounds)
						}
						if group.Bounds.X < stamp.Bounds.X+stamp.Bounds.W-1 || group.Bounds.X > stamp.Bounds.X+stamp.Bounds.W+10 {
							t.Fatal("application must immediately follow device/time")
						}
						if math.Abs(float64(icon.Bounds.Y+icon.Bounds.H/2-stamp.Bounds.Y-stamp.Bounds.H/2)) > 1 {
							t.Fatal("application icon and time must align")
						}
						if group.Bounds.X+group.Bounds.W > detail.Bounds.X+detail.Bounds.W || name.Bounds.X+name.Bounds.W > group.Bounds.X+group.Bounds.W+1 {
							t.Fatal("long application name escapes detail header")
						}
						if sample.icon {
							frame := tt.Image()
							scale := float32(frame.Bounds().Dx()) / float32(width)
							pixel := frame.RGBAAt(int((icon.Bounds.X+8)*scale), int((icon.Bounds.Y+8)*scale))
							if pixel.R < 200 || pixel.G > 60 || pixel.B < 60 || pixel.B > 110 {
								t.Fatalf("cached source icon was not rendered: %+v", pixel)
							}
						}
					})
				}
			}
			t.Run(skin+"-"+theme+"-selection", func(t *testing.T) {
				t.Setenv("UNIC_UI_SKIN", skin)
				d := newDemo("history", theme)
				d.language = "zh-CN"
				a := projectHistory(api.HistoryItem{ID: "first", Kind: "text", SourceApplication: application("Example Editor", encoded.Bytes()), CreatedAt: time.Now().UnixMilli()})
				a.title = "With application"
				a.text = "First fixture"
				b := projectHistory(api.HistoryItem{ID: "second", Kind: "text", CreatedAt: time.Now().UnixMilli()})
				b.title = "Without application"
				b.text = "Second fixture"
				d.entries = []entry{a, b}
				d.selectEntry(a.id)
				tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
				proof.Capture(t, tt)
				if _, ok := tt.AccessByRole(ui.RoleGroup, "来源应用"); !ok {
					t.Fatal("initial source missing")
				}
				if err := tt.Click("Without application"); err != nil {
					t.Fatal(err)
				}
				tt.Advance(200 * time.Millisecond)
				if d.selected != b.id {
					t.Fatal("record selection did not change")
				}
				if _, ok := tt.AccessByRole(ui.RoleGroup, "来源应用"); ok {
					t.Fatal("previous application's identity remains after selection change")
				}
			})
		}
	}
}
