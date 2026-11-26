package paths

import (
	"os"
	"path/filepath"
	"testing"
)

func TestValidateProfileName(t *testing.T) {
	for _, ok := range []string{"", "default", "work", "lab-1", "a_b.c", "A9"} {
		if err := ValidateProfileName(ok); err != nil {
			t.Fatalf("expected %q to be valid, got %v", ok, err)
		}
	}
	for _, bad := range []string{"../evil", "a/b", `a\b`, "-dash", ".dot", "a..b", "with space", "abcdefghijklmnopqrstuvwxyz0123456"} {
		if err := ValidateProfileName(bad); err == nil {
			t.Fatalf("expected %q to be rejected", bad)
		}
	}
}

func TestProfileResolutionPrecedence(t *testing.T) {
	t.Cleanup(func() { _ = SetProfile("") })
	tmp := t.TempDir()
	homeOverride := filepath.Join(tmp, "custom-home")
	t.Setenv("UNIC_HOME", homeOverride)
	t.Setenv("UNIC_SOCK", filepath.Join(tmp, "custom.sock"))
	t.Setenv("UNIC_PROFILE", "from-env")

	// UNIC_HOME beats UNIC_PROFILE when --profile is unset.
	dir, err := ResolveDir()
	if err != nil || dir != homeOverride {
		t.Fatalf("expected UNIC_HOME %q, got %q (%v)", homeOverride, dir, err)
	}
	sock, err := ResolveSocket()
	if err != nil || sock != filepath.Join(tmp, "custom.sock") {
		t.Fatalf("expected UNIC_SOCK, got %q (%v)", sock, err)
	}

	// Explicit --profile beats UNIC_HOME and ignores UNIC_SOCK.
	if err := SetProfile("work"); err != nil {
		t.Fatal(err)
	}
	dir, err = ResolveDir()
	if err != nil || filepath.Base(dir) != baseDirName()+"-work" {
		t.Fatalf("expected named profile dir, got %q (%v)", dir, err)
	}
	sock, err = ResolveSocket()
	if err != nil || sock != filepath.Join(dir, "unic.sock") {
		t.Fatalf("expected profile socket %q, got %q (%v)", filepath.Join(dir, "unic.sock"), sock, err)
	}
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("ResolveDir/ResolveSocket should not create directory")
	}

	// Clearing --profile and UNIC_HOME falls back to UNIC_PROFILE.
	if err := SetProfile(""); err != nil {
		t.Fatal(err)
	}
	t.Setenv("UNIC_HOME", "")
	dir, err = ResolveDir()
	if err != nil || filepath.Base(dir) != baseDirName()+"-from-env" {
		t.Fatalf("expected UNIC_PROFILE dir, got %q (%v)", dir, err)
	}
}
