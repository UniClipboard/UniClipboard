package headless

import (
	"context"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

// TestProductionMigrationResilience is the canonical process-suite adapter for
// the release migration runner. The Python runner owns its redacted receipt,
// source/program hashes, bounded-resource assertions and private-data cleanup.
func TestProductionMigrationResilience(t *testing.T) {
	if os.Getenv("UNIC_MIGRATION_RESILIENCE_E2E") == "" {
		t.Skip("set UNIC_MIGRATION_RESILIENCE_E2E=1")
	}
	if runtime.GOOS == "windows" {
		t.Skip("Unix process/RSS runner; Windows native acceptance remains separate")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Minute)
	defer cancel()
	root, err := filepath.Abs("../..")
	need(t, err)
	bin := t.TempDir()
	for _, name := range []string{"unic", "unicd"} {
		cmd := exec.CommandContext(ctx, "go", "build", "-o", filepath.Join(bin, name), "./app/service/cmd/"+name)
		cmd.Dir = root
		out, err := cmd.CombinedOutput()
		if err != nil {
			t.Fatalf("production build: %v %s", err, out)
		}
	}
	artifacts := os.Getenv("UNIC_MIGRATION_RESILIENCE_ARTIFACTS")
	if artifacts == "" {
		artifacts = t.TempDir()
	}
	args := []string{filepath.Join(root, "e2e/release/migration_resilience.py"), "--cli", filepath.Join(bin, "unic"), "--daemon", filepath.Join(bin, "unicd"), "--artifacts", artifacts}
	if limited := os.Getenv("UNIC_MIGRATION_LIMITED_DIR"); limited != "" {
		args = append(args, "--limited-dir", limited)
	}
	cmd := exec.CommandContext(ctx, "python3", args...)
	out, err := cmd.CombinedOutput()
	t.Logf("%s", out)
	if err != nil {
		t.Fatal(err)
	}
}
