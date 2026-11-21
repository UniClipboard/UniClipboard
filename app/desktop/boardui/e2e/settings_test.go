package e2e_test

// Failure inventory and protocol were written in docs/exec/boardui-mygo-implementation.md
// before component code. These scenarios exercise the complete public native form.
import (
	"encoding/json"
	"image/png"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui/showcase"
)

func scenarioSettingsEndToEnd(t *testing.T) {
	for _, dark := range []bool{false, true} {
		for _, scale := range []float32{1, 2} {
			name := "light"
			if dark {
				name = "dark"
			}
			name += map[float32]string{1: "-1x", 2: "-2x"}[scale]
			t.Run(name, func(t *testing.T) {
				a := showcase.New()
				a.Dark = dark
				started := time.Now()
				tt := ui.NewTesterWith(a.View, 800, 760, ui.TesterOptions{Dark: dark})
				tt.SetLocale("en-US")
				tt.SetScale(scale)
				first := time.Since(started)
				t.Cleanup(func() {
					if t.Failed() {
						capture(t, tt, name+"-failure")
					}
				})
				capture(t, tt, name+"-launcher")
				mustClick(t, tt, "Open settings")
				tt.Advance(400 * time.Millisecond)
				capture(t, tt, name+"-dialog")
				mustAccess(t, tt, ui.RoleDialog, "Workspace settings")
				mustClick(t, tt, "Role")
				tt.Advance(200 * time.Millisecond)
				capture(t, tt, name+"-select")
				tt.Key(0, ui.KeyDown)
				tt.Key(0, ui.KeyEnter)
				if a.Role != "admin" {
					t.Fatalf("keyboard selected role=%q", a.Role)
				}
				if !tt.Focused("Role") {
					t.Fatal("selection did not restore trigger focus")
				}
				mustClick(t, tt, "Role")
				tt.Advance(200 * time.Millisecond)
				capture(t, tt, name+"-select-selected")
				tt.Key(0, ui.KeyEscape)
				if !a.Open || a.RoleOpen {
					t.Fatal("nested Escape closed wrong scope")
				}
				mustClick(t, tt, "Name")
				tt.Command("selectAll")
				tt.Type("Maya 中文 🙂")
				if a.Name != "Maya 中文 🙂" {
					t.Fatalf("edited name=%q", a.Name)
				}
				mustClick(t, tt, "Read-only")
				tt.Command("selectAll")
				tt.Type("forbidden")
				if a.ReadOnly != "Managed by workspace" {
					t.Fatal("read-only input changed")
				}
				if !mustAccess(t, tt, ui.RoleTextField, "Read-only").ReadOnly {
					t.Fatal("read-only semantics missing")
				}
				if !mustAccess(t, tt, ui.RoleTextField, "Disabled").Disabled {
					t.Fatal("disabled semantics missing")
				}
				if !mustAccess(t, tt, ui.RoleTextField, "Password").Password {
					t.Fatal("password semantics missing")
				}
				mustClick(t, tt, "Name")
				tt.Command("selectAll")
				tt.Type("")
				tt.Key(0, ui.KeyBackspace)
				mustClick(t, tt, "Save changes")
				invalidName := mustAccess(t, tt, ui.RoleTextField, "Name")
				if !invalidName.Invalid || len(invalidName.ErrorMessage) != 1 || invalidName.ErrorMessage[0] != "Name is required." {
					t.Fatalf("invalid/error relation not exposed: %+v", invalidName)
				}
				tt.Advance(450 * time.Millisecond)
				capture(t, tt, name+"-invalid")
				mustClick(t, tt, "Name")
				tt.Type("Maya 中文 🙂")
				for i := 0; i < 14; i++ {
					tt.Key(0, ui.KeyTab)
					if tt.Focused("Open settings") {
						t.Fatal("Tab escaped dialog")
					}
				}
				for i := 0; i < 14; i++ {
					tt.Key(ui.Shift, ui.KeyTab)
					if tt.Focused("Open settings") {
						t.Fatal("Shift+Tab escaped dialog")
					}
				}
				tt.Key(0, ui.KeyEscape)
				tt.Advance(time.Second)
				if a.Open {
					t.Fatal("dialog did not close")
				}
				if !tt.Focused("Open settings") {
					t.Fatal("dialog did not restore opener focus")
				}
				mustClick(t, tt, "Open settings")
				tt.Advance(400 * time.Millisecond)
				mustClick(t, tt, "Save changes")
				tt.Advance(time.Second)
				if a.Open || a.Saved != 1 || a.Name != "Maya 中文 🙂" || a.Role != "admin" {
					t.Fatalf("save/persistence failed: %+v", a)
				}
				capture(t, tt, name+"-saved")
				mustClick(t, tt, "Open settings")
				tt.Advance(400 * time.Millisecond)
				measure(t, tt, first, name)
				mustClick(t, tt, "Role")
				tt.Advance(200 * time.Millisecond)
				cancel := mustAccess(t, tt, ui.RoleButton, "Cancel")
				// The popup overlaps the left edge of Cancel. Click its visible
				// right interior, not a point covered by popup padding.
				tt.ClickAt(cancel.Bounds.X+cancel.Bounds.W-4, cancel.Bounds.Y+cancel.Bounds.H/2)
				tt.Advance(400 * time.Millisecond)
				if a.Open || a.RoleOpen {
					t.Fatal("non-modal outside Cancel was swallowed")
				}
				if !tt.Focused("Open settings") {
					t.Fatal("outside Cancel did not restore opener")
				}
			})
		}
	}
}

func scenarioNarrowReducedMotionEndToEnd(t *testing.T) {
	for _, dark := range []bool{false, true} {
		a := showcase.New()
		a.Open = true
		a.Dark = dark
		tt := ui.NewTesterWith(a.View, 360, 720, ui.TesterOptions{Dark: dark, Preferences: ui.Preferences{ReduceMotion: true}})
		tt.SetLocale("en-US")
		stem := "narrow"
		if dark {
			stem = "narrow-dark"
		}
		capture(t, tt, stem+"-1x-dialog")
		mustAccess(t, tt, ui.RoleDialog, "Workspace settings")
		mustClick(t, tt, "Role")
		tt.Advance(200 * time.Millisecond)
		capture(t, tt, stem+"-select")
		for _, n := range tt.AccessAll() {
			if n.Role == ui.RoleOption && !n.Offscreen && (n.Bounds.X < 0 || n.Bounds.X+n.Bounds.W > 360.5) {
				t.Fatalf("option outside window: %+v", n)
			}
		}
		tt.Key(0, ui.KeyEnd)
		tt.Key(0, ui.KeyEnter)
		if a.Role != "viewer" {
			t.Fatal("End/Enter failed")
		}
		capture(t, tt, stem+"-settled")
	}
}

func mustClick(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	for _, role := range []ui.Role{ui.RoleTextField, ui.RolePopUpButton, ui.RoleButton} {
		if a, ok := tt.AccessByRole(role, name); ok {
			if a.Disabled {
				t.Fatalf("attempted disabled control %q", name)
			}
			if a.Bounds.W <= 0 || a.Bounds.H <= 0 {
				t.Fatalf("control %q has no visible bounds", name)
			}
			tt.ClickAt(a.Bounds.X+a.Bounds.W/2, a.Bounds.Y+a.Bounds.H/2)
			return
		}
	}
	if err := tt.Click(name); err != nil {
		t.Fatal(err)
	}
}
func mustAccess(t *testing.T, tt *ui.Tester, role ui.Role, name string) ui.AccessInfo {
	t.Helper()
	a, ok := tt.AccessByRole(role, name)
	if !ok {
		t.Fatalf("missing %v %q; tree=%+v", role, name, tt.AccessAll())
	}
	return a
}
func capture(t *testing.T, tt *ui.Tester, name string) {
	t.Helper()
	root := os.Getenv("UNIC_DESKTOP_E2E_CASE_ARTIFACTS")
	if root == "" {
		root = os.Getenv("BOARDUI_E2E_ARTIFACTS")
	}
	if root == "" {
		t.Fatal("run make desktop-e2e CASE=boardui-settings")
	}
	if err := os.MkdirAll(root, 0755); err != nil {
		t.Fatal(err)
	}
	f, err := os.Create(filepath.Join(root, name+".png"))
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
	b, err := json.MarshalIndent(tt.AccessAll(), "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(root, name+"-access.json"), b, 0644); err != nil {
		t.Fatal(err)
	}
}
