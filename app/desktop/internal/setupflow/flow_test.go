package setupflow

import (
	"errors"
	"os"
	"path/filepath"
	"testing"
)

func TestCheckAdmissionReadOnly(t *testing.T) {
	base := t.TempDir()

	// Missing directory: rejected without creating it.
	missing := filepath.Join(base, "missing")
	if err := CheckAdmission(missing); !errors.Is(err, ErrSetupRequired) {
		t.Fatalf("missing dir: got %v, want ErrSetupRequired", err)
	}
	if _, err := os.Stat(missing); !os.IsNotExist(err) {
		t.Fatal("CheckAdmission created missing directory")
	}

	// Empty directory: rejected without creating desktop-setup.json.
	empty := filepath.Join(base, "empty")
	if err := os.MkdirAll(empty, 0700); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(empty); !errors.Is(err, ErrSetupRequired) {
		t.Fatalf("empty dir: got %v, want ErrSetupRequired", err)
	}
	if _, err := os.Stat(filepath.Join(empty, "desktop-setup.json")); !os.IsNotExist(err) {
		t.Fatal("CheckAdmission created desktop-setup.json in empty directory")
	}

	// Incomplete desktop-setup.json: rejected.
	incomplete := filepath.Join(base, "incomplete")
	if err := os.MkdirAll(incomplete, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(incomplete, "desktop-setup.json"), []byte(`{"complete":false}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(incomplete); !errors.Is(err, ErrSetupRequired) {
		t.Fatalf("incomplete setup: got %v, want ErrSetupRequired", err)
	}

	// Pending CLI migration overrides completed desktop-setup.json: rejected.
	pending := filepath.Join(base, "pending")
	if err := os.MkdirAll(pending, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pending, "desktop-setup.json"), []byte(`{"complete":true}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(pending, "cli-migration.json"), []byte(`{"status":"pending"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(pending); !errors.Is(err, ErrMigrationPending) {
		t.Fatalf("pending migration: got %v, want ErrMigrationPending", err)
	}

	// Completed desktop-setup.json: admitted.
	complete := filepath.Join(base, "complete")
	if err := os.MkdirAll(complete, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(complete, "desktop-setup.json"), []byte(`{"complete":true}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(complete); err != nil {
		t.Fatalf("complete setup: unexpected err %v", err)
	}

	// CLI-initialized profile without desktop-setup.json: admitted read-only.
	cliOnly := filepath.Join(base, "cli-only")
	if err := os.MkdirAll(cliOnly, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(cliOnly, "cli-migration.json"), []byte(`{"status":"complete","outcome":"no_source"}`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(cliOnly); err != nil {
		t.Fatalf("cli-only profile: unexpected err %v", err)
	}
	if _, err := os.Stat(filepath.Join(cliOnly, "desktop-setup.json")); !os.IsNotExist(err) {
		t.Fatal("CheckAdmission created desktop-setup.json in cli-only profile")
	}

	// Established v2 profile (nonempty unic.db, no marker): admitted read-only.
	established := filepath.Join(base, "established")
	if err := os.MkdirAll(established, 0700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(established, "unic.db"), []byte("sqlite"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := CheckAdmission(established); err != nil {
		t.Fatalf("established profile: unexpected err %v", err)
	}
	if _, err := os.Stat(filepath.Join(established, "desktop-setup.json")); !os.IsNotExist(err) {
		t.Fatal("CheckAdmission created desktop-setup.json in established profile")
	}
}
