package engine

import (
	"crypto/ed25519"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/internal/identity"
)

func remoteKey(t *testing.T, b byte) []byte {
	t.Helper()
	id, err := identity.FromSeed([]byte(strings.Repeat(string([]byte{b}), ed25519.SeedSize)))
	if err != nil {
		t.Fatal(err)
	}
	return id.Public()
}

func TestDeviceListShowsOneDeviceWithIndependentPaths(t *testing.T) {
	n := newNode(t, nil)
	sub, err := n.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()

	pk := remoteKey(t, 5)
	d, err := n.e.registerVerifiedDevice(pk, "Bob's phone")
	if ev, nerr := sub.Next(bg); nerr != nil || ev.Kind != EventDevices || len(ev.Objects) != 1 || ev.Objects[0].Type != "device" || ev.Objects[0].ID != d.ID {
		t.Fatalf("event %+v %v", ev, nerr)
	}
	if err != nil || d.Trust != DeviceTrustPending || d.DisplayName() != "Bob's phone" {
		t.Fatalf("%+v %v", d, err)
	}
	// Seen again through another path: still one device.
	if again, _ := n.e.registerVerifiedDevice(pk, "Bob's phone"); again.ID != d.ID {
		t.Fatal("device duplicated")
	}
	b1, err := n.e.bindDevice(d.ID, "builtin.tailcat", []byte("peer-1"))
	if err != nil {
		t.Fatal(err)
	}
	b2, _ := n.e.bindDevice(d.ID, "webdav.work", []byte("repo-1"))
	if ds, _ := n.e.ListDevices(bg); len(ds) != 1 || len(ds[0].Bindings) != 2 {
		t.Fatalf("%+v", ds)
	}

	// A path cannot be authorized for a device nobody trusts.
	if _, err := n.e.AuthorizeBinding(bg, BindingRequest{ID: b1.ID, ExpectedRevision: b1.Revision}); err == nil {
		t.Fatal("authorized a path of an untrusted device")
	}
	// Trust needs the fingerprint the user compared, and it is bound to this device.
	if _, err := n.e.TrustDevice(bg, TrustDeviceRequest{ID: d.ID, ExpectedRevision: d.Revision, ExpectedFingerprint: "0000 0000"}); code(err) != CodeIdentityMismatch {
		t.Fatalf("wrong fingerprint: %v", err)
	}
	want := identity.Fingerprint(n.e.ident.Public(), pk)
	d, err = n.e.TrustDevice(bg, TrustDeviceRequest{ID: d.ID, ExpectedRevision: d.Revision, ExpectedFingerprint: strings.ToLower(want)})
	if err != nil || d.Trust != DeviceTrustTrusted {
		t.Fatalf("%+v %v", d, err)
	}
	// Trusting a device grants no path.
	for _, b := range d.Bindings {
		if b.State != BindingPending {
			t.Fatalf("path authorized by trusting: %+v", b)
		}
	}
	a1, err := n.e.AuthorizeBinding(bg, BindingRequest{ID: b1.ID, ExpectedRevision: b1.Revision})
	if err != nil || a1.State != BindingAuthorized {
		t.Fatalf("%+v %v", a1, err)
	}
	if _, err := n.e.AuthorizeBinding(bg, BindingRequest{ID: b1.ID, ExpectedRevision: b1.Revision}); code(err) != CodeConflict {
		t.Fatalf("stale: %v", err)
	}
	// Pausing and removing one path leave the other untouched.
	if p, err := n.e.SetBindingSync(bg, SetBindingSyncRequest{ID: b1.ID, ExpectedRevision: a1.Revision, Enabled: false}); err != nil || p.SyncEnabled {
		t.Fatalf("%+v %v", p, err)
	}
	if err := n.e.RemoveBinding(bg, BindingRequest{ID: b2.ID, ExpectedRevision: b2.Revision}); err != nil {
		t.Fatal(err)
	}
	got, _ := n.e.GetDevice(bg, d.ID)
	if len(got.Bindings) != 1 || got.Bindings[0].ID != b1.ID || got.Bindings[0].State != BindingAuthorized {
		t.Fatalf("%+v", got.Bindings)
	}

	// Renaming is the user's; the device's own name never overwrites it.
	named, err := n.e.RenameDevice(bg, RenameDeviceRequest{ID: d.ID, ExpectedRevision: got.Revision, Name: "Bob"})
	if err != nil || named.DisplayName() != "Bob" {
		t.Fatalf("%+v %v", named, err)
	}
	if _, err := n.e.registerVerifiedDevice(pk, "Evil name"); err != nil {
		t.Fatal(err)
	}
	if g, _ := n.e.GetDevice(bg, d.ID); g.DisplayName() != "Bob" || g.Trust != DeviceTrustTrusted {
		t.Fatalf("%+v", g)
	}
	if _, err := n.e.RenameDevice(bg, RenameDeviceRequest{ID: d.ID, ExpectedRevision: named.Revision, Name: "bad\nname"}); code(err) != CodeInvalidArgument {
		t.Fatal(err)
	}

	// Unpair removes every path and the device from the list.
	cur, _ := n.e.GetDevice(bg, d.ID)
	if err := n.e.UnpairDevice(bg, UnpairDeviceRequest{ID: d.ID, ExpectedRevision: cur.Revision}); err != nil {
		t.Fatal(err)
	}
	if ds, _ := n.e.ListDevices(bg); len(ds) != 0 {
		t.Fatalf("%+v", ds)
	}
	if _, err := n.e.GetDevice(bg, d.ID); code(err) != CodeNotFound {
		t.Fatal(err)
	}
	// Rediscovery needs a new approval.
	if r, _ := n.e.registerVerifiedDevice(pk, "Bob's phone"); r.Trust != DeviceTrustPending || len(r.Bindings) != 0 {
		t.Fatalf("%+v", r)
	}
}

func TestDeviceIdentityConflictIsRefused(t *testing.T) {
	n := newNode(t, nil)
	d, _ := n.e.registerVerifiedDevice(remoteKey(t, 6), "x")
	// The same key can never register under another id (ids derive from keys),
	// and a different key gets its own id: no record is ever overwritten.
	o, _ := n.e.registerVerifiedDevice(remoteKey(t, 7), "x")
	if o.ID == d.ID {
		t.Fatal("two keys, one device")
	}
	if ds, _ := n.e.ListDevices(bg); len(ds) != 2 {
		t.Fatalf("%+v", ds)
	}
}

// registerVerifiedDevice and bindDevice seed the store the way a verified
// handshake would, without a network.
func (e *Engine) registerVerifiedDevice(publicKey []byte, reportedName string) (Device, error) {
	d, err := e.st.AddDevice(identity.DeviceID(publicKey), publicKey, reportedName)
	if err != nil {
		return Device{}, deviceStoreErr(err)
	}
	e.deviceChanged(d.ID)
	return e.loadDevice(d.ID)
}

func (e *Engine) bindDevice(deviceID, instanceID string, remoteContext []byte) (DeviceBinding, error) {
	b, err := e.st.AddBinding(randHex(16), deviceID, instanceID, remoteContext)
	if err != nil {
		return DeviceBinding{}, deviceStoreErr(err)
	}
	e.deviceChanged(deviceID)
	return e.toBinding(b), nil
}
