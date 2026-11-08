package engine

import (
	"bytes"
	"context"
	"io"
	"testing"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Send/Receive make fakePeer a loopback transport: Send authorizes, reads the
// parts and feeds them to the other fake's receiver.
func (f *fakePeer) SetPeerReceiver(r plugin.PeerReceiver) { f.recv = r }

type bodies struct {
	d [][]byte
	i int
}

func (b *bodies) Next(context.Context) (int, io.ReadCloser, error) {
	if b.i >= len(b.d) {
		return 0, nil, io.EOF
	}
	b.i++
	return b.i - 1, io.NopCloser(bytes.NewReader(b.d[b.i-1])), nil
}

func (f *fakePeer) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) error {
	f.sends.Add(1)
	if f.hold != nil {
		<-f.hold
	}
	g, err := src.Authorize(ctx, rpc.Authorize{DispatchID: s.DispatchID, ContentHandle: s.ContentHandle, ConnectionID: s.ConnectionID, SessionToken: s.SessionToken})
	if err != nil {
		return err
	}
	var data [][]byte
	for i := range s.Clip.Parts {
		r, err := src.OpenPart(ctx, g.GrantID, i)
		if err != nil {
			return err
		}
		b, _ := io.ReadAll(r)
		_ = r.Close()
		data = append(data, b)
	}
	if _, err := src.OpenPart(ctx, g.GrantID, 0); err == nil {
		return rpc.NewError(rpc.CodeInternal) // a part opens once
	}
	peer := f.peer
	gr, err := peer.recv.OpenReceive(ctx, rpc.ReceiveOpen{ConnectionID: f.peerConn, SessionToken: f.peerTok, Clip: s.Clip})
	if err != nil || gr.Decision != rpc.DecisionAccept {
		f.lastErr, f.lastDec = err, gr.Decision
		return err
	}
	_, err = peer.recv.Receive(ctx, *gr.ReceiveID, &bodies{d: data})
	return err
}

func TestPeerSendReceiveLoopback(t *testing.T) {
	na, nb, fa, fb := allowedPair(t)
	idA, idB := na.e.ident.DeviceID(), nb.e.ident.DeviceID()
	// b trusts a as well
	da, _ := nb.e.st.Device(idA)
	if _, err := nb.e.st.TrustDevice(idA, da.Revision); err != nil {
		t.Fatal(err)
	}
	bd, err := nb.e.st.InstanceBinding(idA, "test.peer")
	if err != nil {
		t.Fatal(err)
	}
	if _, err := nb.e.st.AuthorizeBinding(bd.ID, bd.Revision); err != nil {
		t.Fatal(err)
	}
	nb.e.syncPeerStates()
	start(t, na)
	start(t, nb)

	bind := []byte("loop")
	dA, eA, dB, eB := runHandshake(t, fa, fb, "loop", nil, bind, bind)
	if eA != nil || eB != nil || dA.Decision != "allow" || dB.Decision != "allow" {
		t.Fatalf("%+v %v / %+v %v", dA, eA, dB, eB)
	}
	fa.peer, fa.peerConn, fa.peerTok = fb, "loop", *dB.SessionToken
	for _, c := range []struct {
		f   *fakePeer
		tok string
	}{{fa, *dA.SessionToken}, {fb, *dB.SessionToken}} {
		if err := c.f.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: "loop", SessionToken: c.tok, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
			t.Fatal(err)
		}
	}
	_ = idB
	id, err := submit(na.e, importText(t, na.e, "hello over peer_sessions"))
	if err != nil {
		t.Fatal(err)
	}
	op := waitSend(t, na.e, id)
	t.Logf("op %+v sends=%d", op, fa.sends.Load())
	if len(op.Targets) != 1 || op.Targets[0].State != TargetSent {
		t.Fatalf("operation: %+v", op)
	}
	eventually(t, "recv", func() bool { return len(received(nb.e)) == 1 || fa.lastDec != "" })
	if fa.lastDec != "" {
		t.Fatalf("decision %q err %v", fa.lastDec, fa.lastErr)
	}
	rs := waitReceived(t, nb.e, 1)
	if string(rs[0].PeerID) != idA {
		t.Fatalf("source: %q", rs[0].PeerID)
	}
	// the same view is not delivered twice
	if fa.sends.Load() != 1 {
		t.Fatalf("sends: %d", fa.sends.Load())
	}
}

func TestPeerReceiveGuards(t *testing.T) {
	na, nb, fa, fb := allowedPair(t)
	idA := na.e.ident.DeviceID()
	da, _ := nb.e.st.Device(idA)
	_, _ = nb.e.st.TrustDevice(idA, da.Revision)
	bd, _ := nb.e.st.InstanceBinding(idA, "test.peer")
	_, _ = nb.e.st.AuthorizeBinding(bd.ID, bd.Revision)
	nb.e.syncPeerStates()
	start(t, nb)
	bind := []byte("g")
	_, _, dB, eB := runHandshake(t, fa, fb, "g", nil, bind, bind)
	if eB != nil || dB.Decision != "allow" {
		t.Fatal(eB, dB)
	}
	hash := "00000000000000000000000000000000000000000000000000000000000000aa"
	clip := func(origin string) rpc.Clip {
		return rpc.Clip{WireID: "w1", Origin: origin, Hash: hash, Parts: []rpc.ClipPart{{Index: 0, Kind: "representation", MIME: "text/plain", Size: 2}}}
	}
	open := func(tok, origin string) (rpc.ReceiveGrant, error) {
		return fb.recv.OpenReceive(bg, rpc.ReceiveOpen{ConnectionID: "g", SessionToken: tok, Clip: clip(origin)})
	}
	if _, err := open(*dB.SessionToken, idA); err != nil {
		t.Fatal(err)
	}
	if _, err := open(string(make([]byte, 0))+"0000000000000000000000000000000000000000000000000000000000000000", idA); errCode(err) != "permission_denied" {
		t.Fatalf("token: %q", errCode(err))
	}
	if g, err := open(*dB.SessionToken, nb.e.ident.DeviceID()); err != nil || g.Decision != rpc.DecisionDiscard {
		t.Fatalf("an origin that is not the proven peer: %+v %v", g, err)
	}
	// unknown receive id
	if _, err := fb.recv.Receive(bg, hash, &bodies{}); errCode(err) != "not_found" {
		t.Fatalf("unknown id: %q", errCode(err))
	}
	// a truncated body gives the reservation back and commits nothing
	g, err := open(*dB.SessionToken, idA)
	if err == nil && g.Decision == rpc.DecisionAccept {
		if _, err := fb.recv.Receive(bg, *g.ReceiveID, &bodies{d: [][]byte{[]byte("x")}}); err == nil {
			t.Fatal("short body accepted")
		}
	}
	if len(received(nb.e)) != 0 {
		t.Fatal("nothing may be listed")
	}
	// revoked authority: discard
	dd, _ := nb.e.st.Device(idA)
	_ = nb.e.st.RevokeDevice(idA, dd.Revision)
	nb.e.syncPeerStates()
	if g, err := open(*dB.SessionToken, idA); err == nil && g.Decision == rpc.DecisionAccept {
		t.Fatal("revoked peer admitted")
	}
}

func TestPeerSendQueueSupersedes(t *testing.T) {
	na, _, fa, fb := allowedPair(t)
	start(t, na)
	bind := []byte("q")
	dA, eA, _, _ := runHandshake(t, fa, fb, "q", nil, bind, bind)
	if eA != nil || dA.Decision != "allow" {
		t.Fatal(eA, dA)
	}
	if err := fa.auth.PeerEvent(bg, rpc.PeerEvent{ConnectionID: "q", SessionToken: *dA.SessionToken, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
		t.Fatal(err)
	}
	fa.hold = make(chan struct{})
	fa.peer, fa.peerConn, fa.peerTok = fb, "q", "x"
	var ids []OperationID
	for _, s := range []string{"one", "two", "three"} {
		id, err := submit(na.e, importText(t, na.e, s))
		if err != nil {
			t.Fatal(err)
		}
		ids = append(ids, id)
		if s == "one" {
			eventually(t, "first send in flight", func() bool { return fa.sends.Load() == 1 })
		}
	}
	// the middle one is replaced by the newest while the first is in flight
	eventually(t, "superseded", func() bool {
		o, _ := na.e.GetOperation(bg, ids[1])
		return len(o.Targets) == 1 && o.Targets[0].State == TargetSuperseded
	})
	close(fa.hold)
	eventually(t, "third dispatched", func() bool { return fa.sends.Load() == 2 })
}
