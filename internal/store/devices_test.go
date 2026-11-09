package store

import (
	"errors"
	"testing"
)

func key(b byte) []byte {
	k := make([]byte, 32)
	for i := range k {
		k[i] = b
	}
	return k
}

func TestDeviceIdentityIsNeverOverwritten(t *testing.T) {
	s, _ := openTemp(t)
	d, err := s.AddDevice("dev-a", key(1), "Alice's Mac")
	if err != nil || d.Trust != TrustPending || d.Revision != 1 {
		t.Fatalf("%+v %v", d, err)
	}
	// Same id, other key; same key, other id.
	if _, err := s.AddDevice("dev-a", key(2), "x"); !errors.Is(err, ErrIdentityConflict) {
		t.Fatalf("same id other key: %v", err)
	}
	if _, err := s.AddDevice("dev-b", key(1), "x"); !errors.Is(err, ErrIdentityConflict) {
		t.Fatalf("same key other id: %v", err)
	}
	if got, _ := s.Device("dev-a"); string(got.PublicKey) != string(key(1)) || got.ReportedName != "Alice's Mac" {
		t.Fatalf("record changed: %+v", got)
	}
	// Discovery does not touch the user's name or trust.
	named, err := s.SetDeviceName("dev-a", 1, "Work Mac")
	if err != nil || named.Name != "Work Mac" || named.Revision != 2 {
		t.Fatalf("%+v %v", named, err)
	}
	if _, err := s.TrustDevice("dev-a", 2); err != nil {
		t.Fatal(err)
	}
	again, _ := s.AddDevice("dev-a", key(1), "Renamed by itself")
	if again.Name != "Work Mac" || again.Trust != TrustTrusted || again.ReportedName != "Renamed by itself" {
		t.Fatalf("%+v", again)
	}
}

func TestDeviceCASAndPersistence(t *testing.T) {
	s, path := openTemp(t)
	_, _ = s.AddDevice("dev-a", key(1), "")
	if _, err := s.SetDeviceName("dev-a", 7, "x"); !errors.Is(err, ErrDeviceConflict) {
		t.Fatal(err)
	}
	if _, err := s.SetDeviceName("nope", 1, "x"); !errors.Is(err, ErrDeviceNotFound) {
		t.Fatal(err)
	}
	_ = s.Close()
	s2, err := Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s2.Close() }()
	if ds, _ := s2.Devices(); len(ds) != 1 || ds[0].ID != "dev-a" {
		t.Fatalf("%+v", ds)
	}
}

func TestBindingsAreIndependentPaths(t *testing.T) {
	s, _ := openTemp(t)
	_, _ = s.AddDevice("dev-a", key(1), "")
	if _, err := s.AddBinding("b0", "ghost", "inst1", nil); !errors.Is(err, ErrDeviceNotFound) {
		t.Fatal(err)
	}
	b1, err := s.AddBinding("b1", "dev-a", "inst1", []byte("ctx1"))
	if err != nil || b1.State != BindingPending || !b1.SyncEnabled {
		t.Fatalf("%+v %v", b1, err)
	}
	b2, _ := s.AddBinding("b2", "dev-a", "inst2", []byte("ctx2"))
	// A binding needs a trusted device before it can be authorized.
	if _, err := s.AuthorizeBinding("b1", 1); err == nil {
		t.Fatal("authorized for an untrusted device")
	}
	_, _ = s.TrustDevice("dev-a", 1)
	a1, err := s.AuthorizeBinding("b1", 1)
	if err != nil || a1.State != BindingAuthorized || a1.Revision != 2 {
		t.Fatalf("%+v %v", a1, err)
	}
	if _, err := s.AuthorizeBinding("b1", 1); !errors.Is(err, ErrBindingConflict) {
		t.Fatalf("stale revision: %v", err)
	}
	// Pausing one path leaves the other alone.
	p, err := s.SetBindingSync("b1", 2, false)
	if err != nil || p.SyncEnabled {
		t.Fatalf("%+v %v", p, err)
	}
	if o, _ := s.Binding(b2.ID); !o.SyncEnabled || o.State != BindingPending {
		t.Fatalf("other path changed: %+v", o)
	}
	// Status reports do not change authorization or revision.
	if err := s.ReportBinding("b1", 123, "unreachable"); err != nil {
		t.Fatal(err)
	}
	if r, _ := s.Binding("b1"); r.Revision != p.Revision || r.LastError != "unreachable" || r.LastActiveMs != 123 {
		t.Fatalf("%+v", r)
	}
	// Redirecting an instance to another context drops its authorization.
	re, err := s.AddBinding("ignored", "dev-a", "inst1", []byte("OTHER repo"))
	if err != nil || re.ID != "b1" || re.State != BindingPending || !re.SyncEnabled || re.Revision <= p.Revision {
		t.Fatalf("%+v %v", re, err)
	}
	same, _ := s.AddBinding("ignored", "dev-a", "inst2", []byte("ctx2"))
	if same.Revision != b2.Revision {
		t.Fatalf("same context bumped revision: %+v", same)
	}
	// Removing one path keeps the device and the other path.
	if err := s.RemoveBinding("b2", b2.Revision); err != nil {
		t.Fatal(err)
	}
	if bs, _ := s.Bindings("dev-a"); len(bs) != 1 || bs[0].ID != "b1" {
		t.Fatalf("%+v", bs)
	}
	if err := s.RemoveBinding("b2", 1); !errors.Is(err, ErrBindingNotFound) {
		t.Fatal(err)
	}
}

func TestRevokeDropsEveryPathAndNeedsNewApproval(t *testing.T) {
	s, _ := openTemp(t)
	_, _ = s.AddDevice("dev-a", key(1), "")
	_, _ = s.TrustDevice("dev-a", 1)
	_, _ = s.AddBinding("b1", "dev-a", "inst1", nil)
	_, _ = s.AddBinding("b2", "dev-a", "inst2", nil)
	if err := s.RevokeDevice("dev-a", 1); !errors.Is(err, ErrDeviceConflict) {
		t.Fatalf("stale: %v", err)
	}
	if err := s.RevokeDevice("dev-a", 2); err != nil {
		t.Fatal(err)
	}
	if bs, _ := s.Bindings("dev-a"); len(bs) != 0 {
		t.Fatalf("bindings survived: %+v", bs)
	}
	if ds, _ := s.Devices(); len(ds) != 0 {
		t.Fatalf("revoked device listed: %+v", ds)
	}
	if _, err := s.TrustDevice("dev-a", 3); err == nil {
		t.Fatal("trusted a revoked device")
	}
	if _, err := s.AddBinding("b3", "dev-a", "inst1", nil); !errors.Is(err, ErrDeviceNotFound) {
		t.Fatalf("bound a revoked device: %v", err)
	}
	// Rediscovery brings it back as pending, never as trusted.
	d, err := s.AddDevice("dev-a", key(1), "")
	if err != nil || d.Trust != TrustPending {
		t.Fatalf("%+v %v", d, err)
	}
	// ...and the tombstone still holds the key.
	if _, err := s.AddDevice("dev-a", key(9), ""); !errors.Is(err, ErrIdentityConflict) {
		t.Fatal(err)
	}
}
