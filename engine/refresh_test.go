package engine

import (
	"context"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin/rpc"
)

type inspectablePeer struct {
	*fakePeer
	mu        sync.Mutex
	snaps     map[string]rpc.PeerLinkStatus
	probeCnt  int
	probePeer []string
}

func (p *inspectablePeer) LinkSnapshots() map[string]rpc.PeerLinkStatus {
	p.mu.Lock()
	defer p.mu.Unlock()
	out := make(map[string]rpc.PeerLinkStatus, len(p.snaps))
	for k, v := range p.snaps {
		out[k] = v
	}
	return out
}

func (p *inspectablePeer) ProbePeers(_ context.Context, peerIDs []string) ([]rpc.PeerLinkStatus, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.probeCnt++
	p.probePeer = append([]string(nil), peerIDs...)
	now := time.Now().UnixMilli()
	var out []rpc.PeerLinkStatus
	for id, st := range p.snaps {
		if len(peerIDs) > 0 && peerIDs[0] != id {
			continue
		}
		st.Path = rpc.LinkPathDirect
		st.LatencyMs = 7
		st.LastCheckedMs = now
		p.snaps[id] = st
		out = append(out, st)
	}
	return out, nil
}

func TestRefreshConnectionsProjectsPathLatencyAndConnecting(t *testing.T) {
	na, nb, fa, fb := allowedPair(t)
	idB := nb.e.ident.DeviceID()
	insp := &inspectablePeer{
		fakePeer: fa,
		snaps: map[string]rpc.PeerLinkStatus{
			idB: {PeerID: idB, State: rpc.LinkStateConnecting},
		},
	}
	na.e.peerProcs[0].pb = insp

	// Before any connected session, inspector's connecting state should project to Connecting.
	ds, err := na.e.ListDevices(bg)
	if err != nil || len(ds) != 1 || len(ds[0].Bindings) != 1 {
		t.Fatalf("ListDevices: %+v %v", ds, err)
	}
	if ds[0].Bindings[0].Connection != Connecting {
		t.Fatalf("Connection = %q, want connecting", ds[0].Bindings[0].Connection)
	}

	// Bring up an authenticated session and run RefreshConnections.
	start(t, na)
	start(t, nb)
	da, _ := nb.e.st.Device(na.e.ident.DeviceID())
	_, _ = nb.e.st.TrustDevice(da.ID, da.Revision)
	if bd, err := nb.e.st.InstanceBinding(da.ID, "test.peer"); err == nil {
		_, _ = nb.e.st.AuthorizeBinding(bd.ID, bd.Revision)
	}
	nb.e.syncPeerStates()
	bind := []byte("refresh-test")
	dA, eA, dB, eB := runHandshake(t, fa, fb, "c1", nil, bind, bind)
	if eA != nil || eB != nil || dA.Decision != "allow" || dB.Decision != "allow" {
		t.Fatalf("handshake failed: %+v %v / %+v %v", dA, eA, dB, eB)
	}
	if err := fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: "c1", SessionToken: *dA.SessionToken, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
		t.Fatal(err)
	}

	refreshed, err := na.e.RefreshConnections(bg, idB)
	if err != nil {
		t.Fatalf("RefreshConnections: %v", err)
	}
	if len(refreshed) != 1 || len(refreshed[0].Bindings) != 1 {
		t.Fatalf("refreshed = %+v", refreshed)
	}
	b := refreshed[0].Bindings[0]
	if b.Connection != Connected || b.ConnectionPath != "direct" || b.LatencyMs != 7 || b.LastCheckedMs == 0 {
		t.Fatalf("unexpected refreshed binding: %+v", b)
	}

	// Unknown device ID must return CodeNotFound.
	if _, err := na.e.RefreshConnections(bg, "nonexistent"); err == nil || asError(err).Code != CodeNotFound {
		t.Fatalf("unknown device err = %v, want not_found", err)
	}
}
