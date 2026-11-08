package engine

import (
	"errors"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/store"
)

// maxPendingDevices bounds the devices that proved an identity but that nobody
// approved. Anyone who can reach this device can make one, so the list is capped.
const maxPendingDevices = 64

var errTooManyPending = errors.New("too many unapproved devices")

// tailcatHost answers the session layer's questions from the unified device
// store, so the session keeps no trust list of its own.
type tailcatHost struct{ e *Engine }

const tailcatInstance = string(BackendTailcat)

func (h tailcatHost) standing(d store.Device, b store.Binding, found bool) peerbackend.Standing {
	s := peerbackend.Standing{DeviceID: d.ID, Name: Device{Name: d.Name, ReportedName: d.ReportedName}.DisplayName()}
	if found {
		s.Allowed = d.Trust == store.TrustTrusted && b.State == store.BindingAuthorized
		s.SyncEnabled = b.SyncEnabled
	}
	return s
}

// Verify records a device whose proof the session layer checked. It trusts
// nothing: a new device and its path both start pending.
func (h tailcatHost) Verify(v peerbackend.Verified) (peerbackend.Standing, error) {
	d, b, err := h.e.recordPeer(tailcatInstance, v)
	if err != nil {
		return peerbackend.Standing{}, err
	}
	return h.standing(d, b, true), nil
}

// recordPeer stores a device whose key a backend proved, and its path on the
// given instance. Nothing becomes trusted: a new device and its binding start
// pending, a revoked one goes back to pending.
func (e *Engine) recordPeer(instance string, v peerbackend.Verified) (store.Device, store.Binding, error) {
	id := identity.DeviceID(v.PublicKey)
	prev, perr := e.st.Device(id)
	known := perr == nil
	if perr != nil && !errors.Is(perr, store.ErrDeviceNotFound) {
		return store.Device{}, store.Binding{}, perr
	}
	if !known || prev.Trust == store.TrustRevoked {
		ds, err := e.st.Devices()
		if err != nil {
			return store.Device{}, store.Binding{}, err
		}
		n := 0
		for _, d := range ds {
			if d.Trust == store.TrustPending {
				n++
			}
		}
		if n >= maxPendingDevices {
			return store.Device{}, store.Binding{}, errTooManyPending
		}
	}
	name := v.Name
	if len(name) > 64 {
		name = ""
	}
	d, err := e.st.AddDevice(id, v.PublicKey, name)
	if err != nil {
		return store.Device{}, store.Binding{}, err
	}
	changed := !known || d.Revision != prev.Revision
	pb, pberr := e.st.InstanceBinding(id, instance)
	b, err := e.st.AddBinding(randHex(16), id, instance, v.Context)
	if err != nil {
		return store.Device{}, store.Binding{}, err
	}
	changed = changed || pberr != nil || pb.Revision != b.Revision
	endpoint := b.Endpoint
	if v.Addr != "" {
		endpoint = []byte(v.Addr)
	}
	ticket := b.Ticket || v.Ticket
	if string(endpoint) != string(b.Endpoint) || ticket != b.Ticket {
		if err := e.st.SetBindingEndpoint(b.ID, endpoint, ticket); err != nil {
			return store.Device{}, store.Binding{}, err
		}
		changed = changed || ticket != b.Ticket
	}
	if changed {
		e.deviceChanged(id)
	}
	b.Endpoint, b.Ticket = endpoint, ticket
	return d, b, nil
}

func (h tailcatHost) Standing(id string) (peerbackend.Standing, bool) {
	d, err := h.e.st.Device(id)
	if err != nil || d.Trust == store.TrustRevoked {
		return peerbackend.Standing{}, false
	}
	b, err := h.e.st.InstanceBinding(id, tailcatInstance)
	return h.standing(d, b, err == nil), err == nil
}

func (h tailcatHost) Targets() []peerbackend.Target {
	bs, err := h.e.st.InstanceBindings(tailcatInstance)
	if err != nil {
		return nil
	}
	var out []peerbackend.Target
	for _, b := range bs {
		if b.State != store.BindingAuthorized || len(b.Endpoint) == 0 {
			continue
		}
		d, err := h.e.st.Device(b.DeviceID)
		if err != nil || d.Trust != store.TrustTrusted {
			continue
		}
		out = append(out, peerbackend.Target{DeviceID: d.ID, Name: Device{Name: d.Name, ReportedName: d.ReportedName}.DisplayName(), Addr: string(b.Endpoint)})
	}
	return out
}

// Pair checks a peer's proof of holding the live ticket and uses it up.
func (h tailcatHost) Pair(t [32]byte, proof []byte) bool { return h.e.pairUsed(t, proof) }

// Active records how the path to a device is doing, as a stable class.
func (h tailcatHost) Active(deviceID string, err error) {
	e := h.e
	b, berr := e.st.InstanceBinding(deviceID, tailcatInstance)
	if berr != nil {
		return
	}
	class, at := "", time.Now().UnixMilli()
	if err != nil {
		class, at = "unreachable", b.LastActiveMs
	}
	if class == b.LastError && err != nil {
		return
	}
	if e.st.ReportBinding(b.ID, at, class) == nil {
		e.deviceChanged(deviceID)
	}
}

// allowedPeer reports whether a device may send us content over tailcat.
func (e *Engine) allowedPeer(id string) (bool, error) { return e.allowedPeerOn(tailcatInstance, id) }

// allowedPeerOn: trusted device with an authorized binding on that backend instance.
func (e *Engine) allowedPeerOn(instance, id string) (bool, error) {
	d, err := e.st.Device(id)
	if errors.Is(err, store.ErrDeviceNotFound) {
		return false, nil
	} else if err != nil {
		return false, err
	}
	b, err := e.st.InstanceBinding(id, instance)
	if errors.Is(err, store.ErrBindingNotFound) {
		return false, nil
	} else if err != nil {
		return false, err
	}
	return d.Trust == store.TrustTrusted && b.State == store.BindingAuthorized, nil
}

// reconcile makes the session layer follow the device store: sessions of
// devices that lost their permission close before this returns.
func (e *Engine) reconcile() {
	if e.core != nil {
		e.core.Reconcile()
	}
	e.syncPeerStates() // out-of-process backends: the same barrier
}
