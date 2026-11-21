package engine

import (
	"fmt"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugins/examples/loopback"
)

// Keep Engine and product acceptance on the same external SDK fixture.
type tcpPeer = loopback.Peer

func e2eInstance() PluginInstance {
	path, digest := procExe()
	return PluginInstance{Process: &PluginProcess{Path: path, SHA256: digest, Args: []string{"-test.run=^$"},
		Env: []string{"UNIC_TEST_PLUGIN=tcppeer"}, Spec: (&tcpPeer{}).Spec()}}
}

// approveDevice is what a user does on the pending request: trust the device
// after comparing fingerprints, then authorize the path.
func approveDevice(t *testing.T, e *Engine, id string) {
	t.Helper()
	eventually(t, "pending device", func() bool {
		d, err := e.GetDevice(bg, id)
		return err == nil && len(d.Bindings) > 0
	})
	d, _ := e.GetDevice(bg, id)
	if _, err := e.TrustDevice(bg, TrustDeviceRequest{ID: id, ExpectedRevision: d.Revision, ExpectedFingerprint: d.Fingerprint}); err != nil {
		t.Fatal(err)
	}
	d, _ = e.GetDevice(bg, id)
	if _, err := e.AuthorizeBinding(bg, BindingRequest{ID: d.Bindings[0].ID, ExpectedRevision: d.Bindings[0].Revision}); err != nil {
		t.Fatal(err)
	}
}

// TestRealProcessesPairAndSync is the end-to-end path: two Engines, each with
// its own backend child process, pair through a ticket, are approved on both
// sides, then clips travel in both directions.
func TestRealProcessesPairAndSync(t *testing.T) {
	newNode := func() node {
		return newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{e2eInstance()} })
	}
	na, nb := newNode(), newNode()
	start(t, na)
	start(t, nb)
	idA, idB := na.e.ident.DeviceID(), nb.e.ident.DeviceID()

	tk, err := na.e.ExportPairingTicketVia(bg, "e2e.tcp")
	if err != nil {
		t.Fatal(err)
	}
	op, err := nb.e.JoinByTicket(bg, JoinTicketRequest{RequestID: "join", Ticket: tk.Ticket})
	if err != nil {
		t.Fatal(err)
	}
	o := waitOp(t, nb.e, op, PhaseSucceeded)
	if o.Pairing == nil || string(o.Pairing.PeerID) != idA || !o.Pairing.AwaitingRemoteApproval {
		t.Fatalf("pairing: %+v", o.Pairing)
	}
	// Nothing flows before both users approve.
	if d, err := na.e.GetDevice(bg, idB); err != nil || d.Trust == "trusted" {
		t.Fatalf("issuer must not trust the joiner yet: %+v %v", d, err)
	}
	approveDevice(t, na.e, idB)
	approveDevice(t, nb.e, idA)

	deliver := func(from, to node, text string) {
		t.Helper()
		// A send that went out on a connection the peer then dropped as the
		// duplicate is lost, as on any transport: send again until it arrives.
		eventually(t, "delivered", func() bool {
			h := importText(t, from.e, text)
			id, err := submit(from.e, h)
			if err != nil {
				return false
			}
			op := waitSend(t, from.e, id)
			if len(op.Targets) != 1 || op.Targets[0].State != TargetSent {
				return false
			}
			for i := 0; i < 20; i++ {
				for _, r := range received(to.e) {
					if string(r.PeerID) == from.e.ident.DeviceID() {
						return true
					}
				}
				time.Sleep(10 * time.Millisecond)
			}
			return false
		})
	}
	deliver(nb, na, "from b")
	deliver(na, nb, "from a")
}

// TestRealProcessesIntroduceDevice: B knows A and C. C learns of A from B, asks
// to pair, both users approve, and A and C then sync directly.
func TestRealProcessesIntroduceDevice(t *testing.T) {
	newNode := func() node {
		return newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{e2eInstance()} })
	}
	na, nb, nc := newNode(), newNode(), newNode()
	for _, n := range []node{na, nb, nc} {
		start(t, n)
	}
	idA, idB, idC := na.e.ident.DeviceID(), nb.e.ident.DeviceID(), nc.e.ident.DeviceID()

	pair := func(issuer, joiner node, iid, jid string) {
		t.Helper()
		tk, err := issuer.e.ExportPairingTicketVia(bg, "e2e.tcp")
		if err != nil {
			t.Fatal(err)
		}
		op, err := joiner.e.JoinByTicket(bg, JoinTicketRequest{RequestID: RequestID("join-" + jid[:6]), Ticket: tk.Ticket})
		if err != nil {
			t.Fatal(err)
		}
		waitOp(t, joiner.e, op, PhaseSucceeded)
		approveDevice(t, issuer.e, jid)
		approveDevice(t, joiner.e, iid)
	}
	pair(na, nb, idA, idB) // B joins A
	pair(nb, nc, idB, idC) // C joins B

	var cand Candidate
	eventually(t, "C is told about A", func() bool {
		cs, err := nc.e.ListCandidates(bg)
		if err != nil {
			t.Fatal(err)
		}
		for _, c := range cs {
			if c.DeviceID == idA {
				cand = c
				return true
			}
		}
		return false
	})
	if len(cand.Introducers) != 1 || cand.Introducers[0].DeviceID != idB {
		t.Fatalf("introducers: %+v", cand.Introducers)
	}
	// A suggestion is not trust: C has no record of A yet.
	if d, err := nc.e.GetDevice(bg, idA); err == nil && d.Trust == "trusted" {
		t.Fatalf("an introduction trusted a device: %+v", d)
	}

	op, err := nc.e.PairCandidate(bg, PairCandidateRequest{RequestID: "cand", DeviceID: idA, BackendID: cand.BackendID})
	if err != nil {
		t.Fatal(err)
	}
	waitOp(t, nc.e, op, PhaseSucceeded)
	approveDevice(t, na.e, idC)
	approveDevice(t, nc.e, idA)

	var last Operation
	n := 0
	t.Cleanup(func() {
		if t.Failed() {
			t.Logf("last operation: %+v", last)
			ds, _ := nc.e.ListDevices(bg)
			t.Logf("C devices: %+v", ds)
		}
	})
	eventually(t, "A–C sync", func() bool {
		n++
		id, err := submit(nc.e, importText(t, nc.e, fmt.Sprint("from c ", n)))
		if err != nil {
			return false
		}
		o := waitSend(t, nc.e, id)
		last = o
		sent := false
		for _, tg := range o.Targets {
			if tg.State == TargetSent && string(tg.PeerID) == idA {
				sent = true
			}
		}
		if !sent {
			return false
		}
		for i := 0; i < 20; i++ {
			for _, r := range received(na.e) {
				if string(r.PeerID) == idC {
					return true
				}
			}
			time.Sleep(10 * time.Millisecond)
		}
		return false
	})
}
