package fixture

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParsePresetAndSpecs(t *testing.T) {
	for _, tc := range []struct {
		in     string
		want   Preset
		texts  int
		images int
		files  int
	}{
		{"empty", PresetEmpty, 0, 0, 0},
		{"STANDARD", PresetStandard, 200, 12, 5},
		{" soak ", PresetSoak, 1000, 30, 10},
	} {
		p, err := ParsePreset(tc.in)
		if err != nil {
			t.Fatalf("ParsePreset(%q): %v", tc.in, err)
		}
		if p != tc.want {
			t.Fatalf("ParsePreset(%q) = %q, want %q", tc.in, p, tc.want)
		}
		spec, err := SpecForPreset(p)
		if err != nil {
			t.Fatalf("SpecForPreset(%q): %v", p, err)
		}
		if spec.TextCount != tc.texts || spec.ImageCount != tc.images || spec.FileCount != tc.files {
			t.Fatalf("SpecForPreset(%q) = %+v", p, spec)
		}
	}
	if _, err := ParsePreset("invalid"); err == nil {
		t.Fatal("expected error for invalid preset")
	}
}

func TestGenerateDeterministicCorpusAndVerify(t *testing.T) {
	dirA := filepath.Join(t.TempDir(), "a")
	dirB := filepath.Join(t.TempDir(), "b")

	opts := []Option{WithCounts(8, 2, 3), WithImageDimensions(64, 48)}
	mA, err := Generate(dirA, PresetStandard, opts...)
	if err != nil {
		t.Fatalf("Generate A: %v", err)
	}
	mB, err := Generate(dirB, PresetStandard, opts...)
	if err != nil {
		t.Fatalf("Generate B: %v", err)
	}

	if mA.ManifestSHA256 != mB.ManifestSHA256 {
		t.Fatalf("manifest SHA mismatch: %s vs %s", mA.ManifestSHA256, mB.ManifestSHA256)
	}
	if mA.ExpectedCount() != 13 {
		t.Fatalf("ExpectedCount = %d, want 13", mA.ExpectedCount())
	}

	cats := map[string]bool{}
	for _, item := range mA.Texts {
		cats[item.Category] = true
	}
	for _, want := range []string{"zh", "en", "code", "link"} {
		if !cats[want] {
			t.Fatalf("missing category %q in generated texts", want)
		}
	}

	if err := mA.Verify(dirA); err != nil {
		t.Fatalf("Verify A: %v", err)
	}
	loaded, err := LoadManifest(dirA)
	if err != nil {
		t.Fatalf("LoadManifest: %v", err)
	}
	if err := loaded.Verify(dirA); err != nil {
		t.Fatalf("Verify loaded: %v", err)
	}

	// Tamper with an image and ensure Verify catches it.
	imgPath := filepath.Join(dirA, filepath.FromSlash(mA.Images[0].RelativePath))
	if err := os.WriteFile(imgPath, []byte("corrupted"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := loaded.Verify(dirA); err == nil {
		t.Fatal("expected Verify to fail after tampering")
	}
}

func TestGenerateEmptyPreset(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "empty")
	m, err := Generate(dir, PresetEmpty)
	if err != nil {
		t.Fatalf("Generate empty: %v", err)
	}
	if m.ExpectedCount() != 0 {
		t.Fatalf("ExpectedCount = %d, want 0", m.ExpectedCount())
	}
	if err := m.Verify(dir); err != nil {
		t.Fatalf("Verify empty: %v", err)
	}
}

func TestBuildBenchmarkConfigTOML(t *testing.T) {
	cfg := buildBenchmarkConfigTOML("lightweight", "en", true)
	for _, want := range []string{
		"[telemetry]\nenabled = false",
		"[sync]\nenabled = false\nautomatic = false\nfiles = false",
		"mode = \"lightweight\"",
		"launch_at_login = false",
		"start_in_background = true",
		"quick_panel_shortcut = \"\"",
		"quick_panel_double_tap = \"\"",
	} {
		if !strings.Contains(cfg, want) {
			t.Fatalf("config missing %q:\n%s", want, cfg)
		}
	}
}

func TestInitProfileRequiresUnicBin(t *testing.T) {
	if _, err := InitProfile(context.Background(), ProfileOptions{}); err == nil {
		t.Fatal("expected error when UnicBin is empty")
	}
}

func TestProfileEnvIsolation(t *testing.T) {
	t.Setenv("UNIC_PROFILE", "prod-user")
	t.Setenv("HOME", "/Users/real-user")
	t.Setenv("LOCALAPPDATA", `C:\Users\real\AppData\Local`)

	p := &Profile{
		HomeDir:      "/tmp/ub-test",
		DiscoveryDir: "/tmp/ub-test-disc",
		SocketPath:   "/tmp/ub-test/unic.sock",
	}
	env := p.Env()
	joined := strings.Join(env, "\n")
	for _, forbidden := range []string{
		"UNIC_PROFILE=",
		"HOME=/Users/real-user",
		`LOCALAPPDATA=C:\Users\real`,
	} {
		if strings.Contains(joined, forbidden) {
			t.Fatalf("Profile.Env leaked %q", forbidden)
		}
	}
	for _, want := range []string{
		"UNIC_HOME=/tmp/ub-test",
		"UNIC_SOCK=/tmp/ub-test/unic.sock",
		"HOME=/tmp/ub-test-disc",
	} {
		if !strings.Contains(joined, want) {
			t.Fatalf("Profile.Env missing %q", want)
		}
	}
}
