package engine

import (
	"errors"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/store"
)

// maxPendingDevices bounds the devices that proved an identity but that nobody
// approved. Anyone who can reach this device can make one, so the list is capped.
const maxPendingDevices = 64

var errTooManyPending = errors.New("too many unapproved devices")

const tailcatInstance = string(BackendTailcat)

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
	e.syncPeerStates() // out-of-process backends: the same barrier
}
