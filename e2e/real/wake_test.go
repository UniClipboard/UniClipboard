package main

import (
	"os"
	"path/filepath"
	"testing"
)

func TestReadLabParallelsAndWakeRefresh(t *testing.T) {
	dir := t.TempDir()
	valid := filepath.Join(dir, "valid.toml")
	src := `version = 1

[nodes.mac]
transport = "local"

[nodes.win]
transport = "parallels"
vm = "Windows 11"

[suite]
topology = "mesh"
repetitions = 1
scenarios = ["wake-refresh"]
text_bytes = [64]
png_pixels = []
file_bytes = []
html_bytes = []
`
	if err := os.WriteFile(valid, []byte(src), 0600); err != nil {
		t.Fatal(err)
	}
	cfg, err := readLab(valid)
	if err != nil {
		t.Fatalf("readLab(valid): %v", err)
	}
	if cfg.Nodes["win"].Transport != "parallels" || cfg.Nodes["win"].VM != "Windows 11" {
		t.Fatalf("unexpected win node: %+v", cfg.Nodes["win"])
	}
	if plannedMeasurements(cfg) != 0 {
		t.Fatalf("plannedMeasurements = %d, want 0 for wake-refresh only", plannedMeasurements(cfg))
	}

	badCases := []struct {
		name string
		toml string
	}{
		{
			name: "parallels missing vm",
			toml: "version = 1\n[nodes.a]\ntransport = \"parallels\"\n[suite]\nscenarios = [\"wake-refresh\"]\ntext_bytes = [16]\npng_pixels = []\nfile_bytes = []\nhtml_bytes = []\n",
		},
		{
			name: "parallels with host",
			toml: "version = 1\n[nodes.a]\ntransport = \"parallels\"\nvm = \"Windows 11\"\nhost = \"x\"\n[suite]\nscenarios = [\"wake-refresh\"]\ntext_bytes = [16]\npng_pixels = []\nfile_bytes = []\nhtml_bytes = []\n",
		},
		{
			name: "local with vm",
			toml: "version = 1\n[nodes.a]\ntransport = \"local\"\nvm = \"Windows 11\"\n[suite]\nscenarios = [\"wake-refresh\"]\ntext_bytes = [16]\npng_pixels = []\nfile_bytes = []\nhtml_bytes = []\n",
		},
	}
	for _, tc := range badCases {
		t.Run(tc.name, func(t *testing.T) {
			p := filepath.Join(dir, tc.name+".toml")
			if err := os.WriteFile(p, []byte(tc.toml), 0600); err != nil {
				t.Fatal(err)
			}
			if _, err := readLab(p); err == nil {
				t.Fatalf("expected validation error for %s", tc.name)
			}
		})
	}
}

func TestHasSensitiveNetworkLeak(t *testing.T) {
	if hasSensitiveNetworkLeak([]byte(`{"peers":[{"bindings":[{"connection":"connected","connection_path":"direct","latency_ms":3}]}]}`)) {
		t.Fatal("clean status JSON flagged as leak")
	}
	if !hasSensitiveNetworkLeak([]byte(`{"addr":"unic-pair:secret"}`)) {
		t.Fatal("pairing ticket not flagged")
	}
	if !hasSensitiveNetworkLeak([]byte(`{"endpoint":"10.211.55.7:41641"}`)) {
		t.Fatal("raw IPv4:port not flagged")
	}
}
