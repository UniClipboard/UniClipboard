package pluginproc

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Inbound scenarios are driven through backend.send: a WireID of "recv:<name>"
// makes the helper behave as if a clip with the same header had arrived.
func (h *peerHelper) SetPeerHost(ph plugin.PeerHost) { h.ph = ph }

func (h *peerHelper) inbound(ctx context.Context, name string, s rpc.Send) error {
	bodies := func(shift uint64) []plugin.ReceiveBody {
		var out []plugin.ReceiveBody
		for _, p := range s.Clip.Parts {
			out = append(out, plugin.ReceiveBody{Size: uint64(p.Size) + shift, R: bytes.NewReader(bytes.Repeat([]byte{'x'}, int(uint64(p.Size)+shift)))})
		}
		return out
	}
	g, err := h.ph.ReceiveOpen(ctx, rpc.ReceiveOpen{ConnectionID: s.ConnectionID, SessionToken: s.SessionToken, Clip: s.Clip})
	if err != nil {
		h.log("open-error")
		return nil
	}
	if g.Decision == rpc.DecisionDiscard {
		h.log("recv discard-at-open")
		return nil
	}
	shift := uint64(0)
	if name == "short" {
		shift = 1
	}
	out, err := h.ph.Receive(ctx, *g.ReceiveID, bodies(shift))
	switch {
	case err != nil:
		h.log("recv-error")
	default:
		h.log("recv " + out)
	}
	if name == "reuse" {
		if _, err := h.ph.Receive(ctx, *g.ReceiveID, bodies(0)); err != nil {
			h.log("reuse-refused")
		} else {
			h.log("reuse-accepted")
		}
	}
	return nil
}

type fakeRecv struct {
	mu       sync.Mutex
	decision string
	opened   int
	got      [][]byte
	outcome  string
}

const testRecvID = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"

func (f *fakeRecv) OpenReceive(_ context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.opened++
	if f.decision == rpc.DecisionDiscard {
		return rpc.ReceiveGrant{Decision: rpc.DecisionDiscard}, nil
	}
	id := testRecvID
	return rpc.ReceiveGrant{Decision: rpc.DecisionAccept, ReceiveID: &id}, nil
}

func (f *fakeRecv) Receive(ctx context.Context, id string, parts plugin.ReceiveParts) (string, error) {
	f.mu.Lock()
	f.got = nil
	f.mu.Unlock()
	for {
		i, body, err := parts.Next(ctx)
		if err == io.EOF {
			break
		}
		if err != nil {
			return "", err
		}
		b, err := io.ReadAll(body)
		_ = body.Close()
		if err != nil {
			return "", err
		}
		f.mu.Lock()
		if i != len(f.got) {
			f.mu.Unlock()
			return "", rpc.NewError(rpc.CodeInternal)
		}
		f.got = append(f.got, b)
		f.mu.Unlock()
	}
	if f.outcome != "" {
		return f.outcome, nil
	}
	return rpc.OutcomeCommit, nil
}

func recvPeer(t *testing.T, r *fakeRecv) (plugin.Plugin, string) {
	p, dir := boundPeer(t)
	p.(plugin.PeerReceiverAware).SetPeerReceiver(r)
	return p, dir
}

func trigger(t *testing.T, p plugin.Plugin, name string, parts ...[]byte) {
	t.Helper()
	s := testSend("recv:"+name, parts...)
	if err := p.(plugin.PeerDispatcher).Send(context.Background(), s, &fakeSource{}); err != nil {
		_ = 0
		t.Fatal(err)
	}
}

func TestPeerReceiveDeliversPartsInOrder(t *testing.T) {
	r := &fakeRecv{}
	p, dir := recvPeer(t, r)
	big := bytes.Repeat([]byte("x"), 640000) // more than one flow-control window
	trigger(t, p, "ok", bytes.Repeat([]byte("x"), 5), big, nil)
	waitLog(t, dir, "recv committed")
	r.mu.Lock()
	defer r.mu.Unlock()
	if len(r.got) != 3 || len(r.got[0]) != 5 || len(r.got[1]) != len(big) || len(r.got[2]) != 0 {
		t.Fatalf("parts: %d", len(r.got))
	}
}

func TestPeerReceiveDiscardAndDiscardedOutcome(t *testing.T) {
	r := &fakeRecv{decision: rpc.DecisionDiscard}
	p, dir := recvPeer(t, r)
	trigger(t, p, "ok", []byte("abc"))
	waitLog(t, dir, "recv discard-at-open")
	r2 := &fakeRecv{outcome: rpc.OutcomeDiscard}
	p2, dir2 := recvPeer(t, r2)
	trigger(t, p2, "ok", []byte("abc"))
	waitLog(t, dir2, "recv discarded")
}

func TestPeerReceiveIDIsSingleUse(t *testing.T) {
	p, dir := recvPeer(t, &fakeRecv{})
	trigger(t, p, "reuse", []byte("abc"))
	waitLog(t, dir, "reuse-refused")
}

func TestPeerReceiveRefusesSizeThatDiffersFromHeader(t *testing.T) {
	p, dir := recvPeer(t, &fakeRecv{})
	trigger(t, p, "short", []byte("abc"))
	waitLog(t, dir, "recv-error")
}

func TestReceiveHandlersCheckGrantsAndOrder(t *testing.T) {
	r := &fakeRecv{}
	p := &Proxy{precv: r}
	ctx := context.Background()
	code := func(err error) string {
		var re *rpc.Error
		if asRPC(err, &re) {
			return re.Code
		}
		return ""
	}
	call := func(parent uint64, v any) *rpc.Call {
		b, _ := jsonMarshal(v)
		return &rpc.Call{ID: 7, Parent: parent, Params: b}
	}
	// A receive id nobody granted.
	if _, err := p.receiveRoot(ctx, call(0, rpc.PeerReceive{ReceiveID: testRecvID})); code(err) != rpc.CodePermission {
		t.Errorf("ungranted id: %v", err)
	}
	// An expired grant is refused and consumed.
	p.rgrants = map[string]*recvGrant{testRecvID: {sizes: []uint64{1}, deadline: time.Now().Add(-time.Second)}}
	if _, err := p.receiveRoot(ctx, call(0, rpc.PeerReceive{ReceiveID: testRecvID})); code(err) != "resource_expired" {
		t.Errorf("expired id: %v", err)
	}
	if len(p.rgrants) != 0 {
		t.Error("expired grant kept")
	}
	// receive_part without a live receive, then out of order and past the end.
	part := func(idx int, size uint64) *rpc.Call {
		return call(7, rpc.PeerReceivePart{ReceiveID: testRecvID, Index: idx, Stream: rpc.StreamDesc{Size: rpc.U64(size), Purpose: rpc.PurposeReceivePart}})
	}
	if _, err := p.receivePart(ctx, part(0, 0)); code(err) != rpc.CodePermission {
		t.Errorf("part without receive: %v", err)
	}
	a := &activeRecv{id: testRecvID, sizes: []uint64{0, 0}, ch: make(chan recvPart, 1)}
	p.recvs = map[uint64]*activeRecv{7: a}
	if _, err := p.receivePart(ctx, part(1, 0)); code(err) != rpc.CodeConflict {
		t.Errorf("out of order: %v", err)
	}
	if _, err := p.receivePart(ctx, part(0, 3)); code(err) != rpc.CodeInvalidArgument {
		t.Errorf("size differs: %v", err)
	}
	a.next = 0 // a size mismatch fails the whole receive; rewind for the next checks
	if _, err := p.receivePart(ctx, part(0, 0)); err != nil {
		t.Errorf("empty part: %v", err)
	}
	_ = (<-a.ch).body.Close()
	if _, err := p.receivePart(ctx, part(0, 0)); code(err) != rpc.CodeConflict {
		t.Errorf("repeated part: %v", err)
	}
	// At most 32 grants outstanding: the 33rd is a discard without asking the Engine.
	p.rgrants = map[string]*recvGrant{}
	for i := 0; i < rpc.MaxOpenReceives; i++ {
		p.rgrants[fmt.Sprintf("%064x", i)] = &recvGrant{deadline: time.Now().Add(time.Minute)}
	}
	open := rpc.ReceiveOpen{ConnectionID: "c1", SessionToken: tok1, Clip: testSend("w", []byte("x")).Clip}
	res, err := p.receiveOpen(ctx, call(0, open))
	if err != nil || res.(rpc.ReceiveGrant).Decision != rpc.DecisionDiscard || r.opened != 0 {
		t.Errorf("33rd grant: %v %v opened=%d", res, err, r.opened)
	}
}
