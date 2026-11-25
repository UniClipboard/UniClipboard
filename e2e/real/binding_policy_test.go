package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestReadLabParallelsBindingPolicy(t *testing.T) {
	cfg, err := readLab("lab.parallels-policy.example.toml")
	if err != nil {
		t.Fatalf("readLab(lab.parallels-policy.example.toml): %v", err)
	}
	if cfg.Nodes["mac"].Transport != "local" {
		t.Fatalf("mac node = %+v", cfg.Nodes["mac"])
	}
	if cfg.Nodes["windows"].Transport != "parallels" || cfg.Nodes["windows"].VM != "Windows 11" {
		t.Fatalf("windows node = %+v", cfg.Nodes["windows"])
	}
	if len(cfg.Suite.Scenarios) != 1 || cfg.Suite.Scenarios[0] != "binding-policy" {
		t.Fatalf("scenarios = %v, want [binding-policy]", cfg.Suite.Scenarios)
	}
	if plannedMeasurements(cfg) != 0 {
		t.Fatalf("plannedMeasurements = %d, want 0 for binding-policy only", plannedMeasurements(cfg))
	}

	// Ensure duplicate binding-policy is rejected.
	dir := t.TempDir()
	dup := filepath.Join(dir, "dup.toml")
	src := "version = 1\n[nodes.a]\ntransport = \"local\"\n[nodes.b]\ntransport = \"parallels\"\nvm = \"Windows 11\"\n[suite]\nscenarios = [\"binding-policy\", \"binding-policy\"]\ntext_bytes = [64]\npng_pixels = []\nfile_bytes = []\nhtml_bytes = []\n"
	if err := os.WriteFile(dup, []byte(src), 0600); err != nil {
		t.Fatal(err)
	}
	if _, err := readLab(dup); err == nil {
		t.Fatal("expected duplicate binding-policy scenario error")
	}
}
