package engine

import (
	"os"
	"path/filepath"
	"testing"
)

func TestInstanceDirPrivateAndLocked(t *testing.T) {
	dir := t.TempDir()
	d, l, err := openInstanceDir(dir, "x")
	if err != nil {
		t.Fatal(err)
	}
	if fi, _ := os.Stat(d); fi.Mode().Perm() != 0o700 {
		t.Fatalf("mode %v", fi.Mode())
	}
	if _, _, err := openInstanceDir(dir, "x"); err == nil {
		t.Fatal("second lock must fail")
	}
	l.release()
	if _, l2, err := openInstanceDir(dir, "x"); err != nil {
		t.Fatal(err)
	} else {
		l2.release()
	}
}

func TestInstanceDirRefusesSymlink(t *testing.T) {
	dir := t.TempDir()
	out := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, "plugin-data"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(out, filepath.Join(dir, "plugin-data", "x")); err != nil {
		t.Fatal(err)
	}
	if _, _, err := openInstanceDir(dir, "x"); err == nil {
		t.Fatal("symlink must be refused")
	}
}
