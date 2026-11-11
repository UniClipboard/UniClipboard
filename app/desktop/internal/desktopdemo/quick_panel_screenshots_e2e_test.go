package desktopdemo

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"image"
	"image/color"
	"image/draw"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"
)

// Failure cases: accidentally starting App.Run; exporting the old desktop
// composition; taking a frame before preview delay; exporting the wrong menu
// or grid state; clipping either window; omitting source identity.
// This opt-in E2E export uses the existing Tester.Image PNG path. Each image
// combines the real history and satellite rasters on a neutral canvas. It
// never opens a native window and is not native window-manager evidence.
func TestQuickPanelScreenshotsE2E(t *testing.T) {
	if os.Getenv("UNIC_QUICK_PANEL_SCREENSHOTS") != "1" {
		t.Skip("set UNIC_QUICK_PANEL_SCREENSHOTS=1 for explicit offscreen screenshot export")
	}
	dir := os.Getenv("UNIC_UI_TEST_ARTIFACTS")
	if dir == "" {
		t.Fatal("UNIC_UI_TEST_ARTIFACTS is required")
	}
	if err := os.MkdirAll(dir, 0755); err != nil {
		t.Fatal(err)
	}
	t.Setenv("UNIC_UI_SKIN", "macos")
	head, err := exec.Command("git", "rev-parse", "HEAD").Output()
	if err != nil {
		t.Fatal(err)
	}
	type screenshot struct {
		File, Theme, Scenario, SHA256 string
		Width, Height, Scale          int
		History, Satellite            ui.Rect
	}
	receipt := struct {
		SourceSHA256       map[string]string
		SourceHEAD         string
		Reference          string
		Method             string
		NativeWindowOpened bool
		Screenshots        []screenshot
	}{SourceHEAD: string(head[:len(head)-1]), Reference: quickPanelReference, Method: "mygo Tester.Image; two separately rendered views composed at 2x scale", NativeWindowOpened: false}
	receipt.SourceSHA256 = map[string]string{}
	sources, err := filepath.Glob("*.go")
	if err != nil {
		t.Fatal(err)
	}
	sources = append(sources, "../../../../go.mod", "../../../../go.sum")
	for _, source := range sources {
		content, err := os.ReadFile(source)
		if err != nil {
			t.Fatal(err)
		}
		hash := sha256.Sum256(content)
		receipt.SourceSHA256[filepath.ToSlash(source)] = hex.EncodeToString(hash[:])
	}
	const scale = 2
	for _, theme := range []string{"light", "dark"} {
		for _, scenario := range []string{"history", "actions", "images"} {
			t.Run(scenario+"-"+theme, func(t *testing.T) {
				p := newQuickDemo(theme, "history")
				options := ui.TesterOptions{Dark: theme == "dark"}
				history := ui.NewTesterWith(p.panel, 380, 366, options)
				history.SetScale(scale)
				history.Advance(510 * time.Millisecond)
				width, height := 400, 240
				switch scenario {
				case "actions":
					history.Key(ui.Cmd, ui.KeyK)
					if p.menu != "main" {
						t.Fatal("actions did not open")
					}
					width, height = 380, 44+len(p.actions())*32+34
				case "images":
					history.Type("/图片")
					history.Key(0, ui.KeyTab)
					history.Advance(510 * time.Millisecond)
					if p.kind != "图片" || p.previewID == "" {
						t.Fatal("image preview missing")
					}
					height = 310
				}
				if p.previewID == "" && p.menu == "" {
					t.Fatal("preview not ready")
				}
				satellite := ui.NewTesterWith(p.satellite, width, height, options)
				satellite.SetScale(scale)
				satellite.Advance(160 * time.Millisecond)
				// The first selected row is near the top. Include the satellite's
				// centered placement even when its top extends above the history.
				previewY := int(p.anchor.Y+p.anchor.H/2) - height/2
				top := min(0, previewY)
				bottom := max(366, previewY+height)
				pad, gap := 16, 8
				canvas := image.NewRGBA(image.Rect(0, 0, (pad*2+380+gap+width)*scale, (pad*2+bottom-top)*scale))
				backdrop := color.RGBA{235, 236, 239, 255}
				if theme == "dark" {
					backdrop = color.RGBA{20, 21, 24, 255}
				}
				draw.Draw(canvas, canvas.Bounds(), image.NewUniform(backdrop), image.Point{}, draw.Src)
				historyPoint := image.Pt(pad*scale, (pad-top)*scale)
				satellitePoint := image.Pt((pad+380+gap)*scale, (pad+previewY-top)*scale)
				panelImage, previewImage := history.Image(), satellite.Image()
				if panelImage.Bounds().Dx() != 380*scale || previewImage.Bounds().Dx() != width*scale {
					t.Fatal("unexpected raster scale")
				}
				draw.Draw(canvas, panelImage.Bounds().Add(historyPoint), panelImage, image.Point{}, draw.Over)
				draw.Draw(canvas, previewImage.Bounds().Add(satellitePoint), previewImage, image.Point{}, draw.Over)
				name := scenario + "-" + theme + ".png"
				file, err := os.Create(filepath.Join(dir, name))
				if err != nil {
					t.Fatal(err)
				}
				if err := png.Encode(file, canvas); err != nil {
					_ = file.Close()
					t.Fatal(err)
				}
				if err := file.Close(); err != nil {
					t.Fatal(err)
				}
				bytes, err := os.ReadFile(filepath.Join(dir, name))
				if err != nil {
					t.Fatal(err)
				}
				hash := sha256.Sum256(bytes)
				receipt.Screenshots = append(receipt.Screenshots, screenshot{File: name, Theme: theme, Scenario: scenario, SHA256: hex.EncodeToString(hash[:]), Width: canvas.Bounds().Dx(), Height: canvas.Bounds().Dy(), Scale: scale, History: ui.Rect{X: float32(historyPoint.X), Y: float32(historyPoint.Y), W: 760, H: 732}, Satellite: ui.Rect{X: float32(satellitePoint.X), Y: float32(satellitePoint.Y), W: float32(width * scale), H: float32(height * scale)}})
			})
		}
	}
	if t.Failed() {
		return
	}
	data, err := json.MarshalIndent(receipt, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "screenshots.json"), append(data, '\n'), 0644); err != nil {
		t.Fatal(err)
	}
}
