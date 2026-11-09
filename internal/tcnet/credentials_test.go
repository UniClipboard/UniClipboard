package tcnet

import (
	"os"
	"path/filepath"
	"runtime"
	"testing"
)

func TestCredentialsAreCreatedOnceAndSurviveRestart(t *testing.T) {
	dir := t.TempDir()
	a, err := loadCredentials(dir)
	if err != nil {
		t.Fatal(err)
	}
	b, err := loadCredentials(dir)
	if err != nil || a != b {
		t.Fatalf("second load differs or failed: %v", err)
	}
	if a.ServerKey == "" || a.ClientKey == "" || a.PSK == "" || a.ServerKey == a.ClientKey {
		t.Fatal("incomplete credentials")
	}
	if runtime.GOOS != "windows" {
		fi, _ := os.Stat(filepath.Join(dir, credFile))
		if fi.Mode().Perm() != 0o600 {
			t.Fatalf("mode %v", fi.Mode())
		}
	}
	// New builds a transport from them and keeps the recorded address.
	a.Addr = "x"
	if err := saveCredentials(dir, a); err != nil {
		t.Fatal(err)
	}
	if c, _ := loadCredentials(dir); c.Addr != "x" || c.ServerKey != a.ServerKey {
		t.Fatalf("%+v", c)
	}
	if _, err := New(dir, func(string, ...any) {}); err != nil {
		t.Fatal(err)
	}
}

func TestBadCredentialsAreRefusedNotReplaced(t *testing.T) {
	good := func(t *testing.T) (string, string) {
		dir := t.TempDir()
		if _, err := loadCredentials(dir); err != nil {
			t.Fatal(err)
		}
		return dir, filepath.Join(dir, credFile)
	}
	for name, mutate := range map[string]func(path string){
		"garbage":    func(p string) { _ = os.WriteFile(p, []byte("not json"), 0o600) },
		"incomplete": func(p string) { _ = os.WriteFile(p, []byte(`{"version":1,"server_key":"a"}`), 0o600) },
		"version":    func(p string) { _ = os.WriteFile(p, []byte(`{"version":2}`), 0o600) },
		"bad key": func(p string) {
			_ = os.WriteFile(p, []byte(`{"version":1,"server_key":"a","psk":"b","client_key":"c"}`), 0o600)
		},
		"symlink": func(p string) { _ = os.Remove(p); _ = os.Symlink("/etc/hosts", p) },
		"wide":    func(p string) { _ = os.Chmod(p, 0o644) },
	} {
		t.Run(name, func(t *testing.T) {
			if runtime.GOOS == "windows" && (name == "wide" || name == "symlink") {
				t.Skip()
			}
			dir, path := good(t)
			mutate(path)
			before, _ := os.Lstat(path)
			if _, err := New(dir, func(string, ...any) {}); err != ErrBadCredentials {
				t.Fatalf("got %v", err)
			}
			after, _ := os.Lstat(path)
			if before.Mode() != after.Mode() || before.Size() != after.Size() {
				t.Fatal("the bad file was changed")
			}
		})
	}
}

func TestNoHalfPublishedCredentials(t *testing.T) {
	dir := t.TempDir()
	// A directory that cannot be written leaves nothing behind.
	if runtime.GOOS == "windows" || os.Getuid() == 0 {
		t.Skip()
	}
	_ = os.Chmod(dir, 0o500)
	defer os.Chmod(dir, 0o700)
	if _, err := loadCredentials(dir); err == nil {
		t.Fatal("expected a write failure")
	}
	_ = os.Chmod(dir, 0o700)
	if ents, _ := os.ReadDir(dir); len(ents) != 0 {
		t.Fatalf("leftovers: %v", ents)
	}
}
