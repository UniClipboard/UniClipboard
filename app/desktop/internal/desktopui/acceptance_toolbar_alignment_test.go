package desktopui

import (
	"encoding/json"
	"math"
	"os"
	"path/filepath"
	"testing"

	"github.com/egoist/mygo/ui"

	proof "github.com/uniclipboard/unic/app/desktop/internal/e2eproof"
)

// Failure case: system-menu triggers align to the capsule's top while the
// adjacent command button stays centered. Drive the complete history view.
func scenarioToolbarAlignment(t *testing.T) {
	t.Setenv("UNIC_UI_SKIN", "macos")
	results := map[string]map[string]ui.Rect{}
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
			if err := os.WriteFile(filepath.Join(dir, "toolbar-alignment.json"), append(b, '\n'), 0644); err != nil {
				t.Error(err)
			}
		}
	}()
	for _, theme := range []string{"dark", "light"} {
		t.Run(theme, func(t *testing.T) {
			d := newDemo("history", theme)
			tt := ui.NewTesterWith(d.view, 1100, 720, ui.TesterOptions{Dark: theme == "dark"})
			proof.Capture(t, tt)
			pin, ok := tt.AccessByRole(ui.RoleButton, "切换置顶")
			if !ok {
				t.Fatal("pin missing")
			}
			results[theme] = map[string]ui.Rect{"pin": pin.Bounds}
			for _, name := range []string{"发送到设备", "更多操作"} {
				a, ok := tt.AccessByRole(ui.RoleMenuButton, name)
				if !ok {
					t.Fatalf("trigger missing: %s", name)
				}
				results[theme][name] = a.Bounds
				if math.Abs(float64(a.Bounds.Y+a.Bounds.H/2-pin.Bounds.Y-pin.Bounds.H/2)) > .1 {
					t.Errorf("%s is not centered with pin: menu=%+v pin=%+v", name, a.Bounds, pin.Bounds)
				}
			}
		})
	}
}
