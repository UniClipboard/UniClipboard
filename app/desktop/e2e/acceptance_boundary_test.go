package e2e

import (
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"
)

// Failure inventory: demo linked into production, shared renderer imports fixtures,
// i18n depends on UI, missing embedded catalogue, broken demo entry, GUI on help.
func scenarioProductionBoundary(t *testing.T) {
	root, err := filepath.Abs("../../..")
	must(t, err)
	checks := []string{}
	run := func(args ...string) string {
		t.Helper()
		cmd := exec.Command("go", args...)
		cmd.Dir = root
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("go %v: %v\n%s", args, err, out)
		}
		return string(out)
	}
	prefix := "github.com/uniclipboard/unic/app/desktop/"
	deps := run("list", "-deps", "./app/desktop/cmd/unic-desktop")
	for _, required := range []string{"internal/desktopui", "internal/i18n"} {
		if !strings.Contains(deps, prefix+required+"\n") {
			t.Fatalf("missing production dependency: %s", required)
		}
	}
	if strings.Contains(deps, prefix+"internal/desktopdemo") {
		t.Fatal("production imports demo code")
	}
	checks = append(checks, "production uses desktopui and i18n without desktopdemo")
	shared := run("list", "-deps", "./app/desktop/internal/desktopui")
	if strings.Contains(shared, prefix+"internal/desktopdemo") {
		t.Fatal("shared renderer imports fixture data")
	}
	checks = append(checks, "shared renderer has no fixture dependency")
	var catalogue struct {
		Imports    []string
		EmbedFiles []string
	}
	must(t, json.Unmarshal([]byte(run("list", "-json", "./app/desktop/internal/i18n")), &catalogue))
	for _, name := range catalogue.Imports {
		if strings.Contains(name, "github.com/") {
			t.Fatalf("translation module has a non-standard dependency: %s", name)
		}
	}
	if len(catalogue.EmbedFiles) != 1 || catalogue.EmbedFiles[0] != "locales.json" {
		t.Fatalf("translation catalogue not embedded: %v", catalogue.EmbedFiles)
	}
	checks = append(checks, "i18n embeds locales.json and depends only on the standard library")
	private := os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE")
	must(t, os.MkdirAll(private, 0700))
	binaries := map[string]string{}
	for _, name := range []string{"unic-desktop", "unic-desktop-demo"} {
		path := filepath.Join(private, name)
		run("build", "-race", "-o", path, "./app/desktop/cmd/"+name)
		binaries[name] = hash(t, path)
	}
	checks = append(checks, "production and demo commands build independently")
	cmd := exec.Command(filepath.Join(private, "unic-desktop-demo"), "--help")
	out, err := cmd.CombinedOutput()
	must(t, err)
	if !strings.Contains(string(out), "-page") || !strings.Contains(string(out), "-theme") {
		t.Fatalf("missing demo help: %s", out)
	}
	checks = append(checks, "demo help exits without opening a native window")
	for _, page := range []string{"setup", "unlock", "startup", "recovery"} {
		cmd := exec.Command(filepath.Join(private, "unic-desktop-demo"), "--page", page)
		out, err := cmd.CombinedOutput()
		if err == nil || !strings.Contains(string(out), "unknown demo page") {
			t.Fatalf("removed page %s accepted: %v %s", page, err, out)
		}
	}
	checks = append(checks, "removed entry pages remain rejected by the independent demo command")

	manifest := map[string]any{"status": "passed", "checks": checks, "binary_sha256": binaries, "native_window_opened": false}
	raw, err := json.MarshalIndent(manifest, "", "  ")
	must(t, err)
	must(t, os.WriteFile(filepath.Join(private, "manifest.json"), raw, 0600))
}
