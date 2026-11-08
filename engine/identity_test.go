package engine

import (
	"os"
	"path/filepath"
	"testing"
)

func TestUnifiedIdentityIsCreatedOnceAndChecked(t *testing.T) {
	n := newNode(t, nil)
	first := n.e.ident.Public()
	if len(first) == 0 {
		t.Fatal("no identity")
	}
	_ = n.e.Close(bg)
	opts, _ := newOpts(t, n.dir)

	e2, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	if string(e2.ident.Public()) != string(first) {
		t.Fatal("identity changed across restarts")
	}
	_ = e2.Close(bg)

	key := filepath.Join(n.dir, "identity", "device.key")
	good, _ := os.ReadFile(key)

	// A registered identity whose key file vanished is not replaced.
	if err := os.Remove(key); err != nil {
		t.Fatal(err)
	}
	if _, err := New(bg, opts); code(err) != CodeUnavailable {
		t.Fatalf("missing key file: %v", err)
	}
	if _, err := os.Stat(key); !os.IsNotExist(err) {
		t.Fatal("a new key was generated in place of the registered one")
	}

	// A different key is an identity mismatch.
	other := newNode(t, nil)
	otherKey, _ := os.ReadFile(filepath.Join(other.dir, "identity", "device.key"))
	_ = other.e.Close(bg)
	if err := os.WriteFile(key, otherKey, 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := New(bg, opts); code(err) != CodeIdentityMismatch {
		t.Fatalf("foreign key: %v", err)
	}

	// A damaged file stops startup and is left alone.
	if err := os.WriteFile(key, []byte("garbage"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := New(bg, opts); code(err) != CodeUnavailable {
		t.Fatalf("damaged key: %v", err)
	}
	if b, _ := os.ReadFile(key); string(b) != "garbage" {
		t.Fatal("damaged file was overwritten")
	}

	// Restoring the original brings the data directory back; the lock was released on each failure.
	if err := os.WriteFile(key, good, 0o600); err != nil {
		t.Fatal(err)
	}
	e3, err := New(bg, opts)
	if err != nil {
		t.Fatalf("restored key: %v", err)
	}
	_ = e3.Close(bg)
}
