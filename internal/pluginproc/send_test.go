package pluginproc

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"strings"
	"sync"
	"testing"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

func (h *peerHelper) Send(ctx context.Context, s rpc.Send, host plugin.SendHost) error {
	if name, ok := strings.CutPrefix(s.Clip.WireID, "recv:"); ok {
		return h.inbound(ctx, name, s)
	}
	g, err := host.Authorize(ctx)
	if err != nil {
		h.log("authorize-refused " + s.DispatchID)
		return err
	}
	sum, total := sha256.New(), 0
	for i := range s.Clip.Parts {
		rc, err := host.OpenPart(ctx, g.GrantID, i)
		if err != nil {
			return err
		}
		n, err := io.Copy(sum, rc)
		rc.Close()
		if err != nil {
			return err
		}
		total += int(n)
	}
	if s.Clip.WireID == "dup" {
		if rc, err := host.OpenPart(ctx, g.GrantID, 0); err == nil {
			rc.Close()
			h.log("dup-accepted")
		} else {
			h.log("dup-refused")
		}
	}
	h.log(fmt.Sprintf("sent %s %d %x", s.DispatchID, total, sum.Sum(nil)[:4]))
	return nil
}

type fakeSource struct {
	mu     sync.Mutex
	parts  [][]byte
	refuse error
	grants int
	opens  int
}

const testGrant = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

func (f *fakeSource) Authorize(context.Context, rpc.Authorize) (rpc.SendGrant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.refuse != nil {
		return rpc.SendGrant{}, f.refuse
	}
	f.grants++
	return rpc.SendGrant{GrantID: testGrant, StateRevision: 7}, nil
}

func (f *fakeSource) OpenPart(_ context.Context, g string, i int) (io.ReadCloser, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if g != testGrant || i >= len(f.parts) {
		return nil, rpc.NewError(rpc.CodeInvalidArgument)
	}
	f.opens++
	return io.NopCloser(bytes.NewReader(f.parts[i])), nil
}

const (
	tok1 = "1111111111111111111111111111111111111111111111111111111111111111"
	tok2 = "2222222222222222222222222222222222222222222222222222222222222222"
)

func testSend(wire string, parts ...[]byte) rpc.Send {
	c := rpc.Clip{WireID: wire, Origin: "origin1", AtMs: 5, Hash: strings.Repeat("0", 64)}
	for i, p := range parts {
		c.Parts = append(c.Parts, rpc.ClipPart{Index: i, Kind: "representation", MIME: "text/plain", Size: rpc.U64(len(p))})
	}
	return rpc.Send{DispatchID: "d1", OperationID: "o1", PeerID: "p1", ConnectionID: "c1", SessionToken: tok1, ContentHandle: tok2,
		Clip: c, BackendID: "example.peer", Mode: "automatic"}
}

func boundPeer(t *testing.T) (plugin.Plugin, string) {
	spec := peerExeSpec(t)
	p, _ := start(t, spec, Options{}, nil)
	if err := p.(plugin.PeerBackend).Bind(context.Background(), testBind()); err != nil {
		t.Fatal(err)
	}
	return p, spec.Dir
}

func TestPeerSendStreamsPartsUnderGrant(t *testing.T) {
	p, dir := boundPeer(t)
	big := bytes.Repeat([]byte("0123456789abcdef"), 40000) // > one flow-control window
	parts := [][]byte{[]byte("hello"), big, {}}
	src := &fakeSource{parts: parts}
	if err := p.(plugin.PeerDispatcher).Send(context.Background(), testSend("w1", parts...), src); err != nil {
		t.Fatal(err)
	}
	sum := sha256.New()
	total := 0
	for _, b := range parts {
		sum.Write(b)
		total += len(b)
	}
	waitLog(t, dir, fmt.Sprintf("sent d1 %d %x", total, sum.Sum(nil)[:4]))
	if src.grants != 1 || src.opens != 3 {
		t.Fatalf("grants=%d opens=%d", src.grants, src.opens)
	}
}

func TestPeerSendRefusedBeforeAnyByte(t *testing.T) {
	p, dir := boundPeer(t)
	src := &fakeSource{parts: [][]byte{[]byte("x")}, refuse: rpc.NewError("source_changed")}
	err := p.(plugin.PeerDispatcher).Send(context.Background(), testSend("w1", []byte("x")), src)
	var re *rpc.Error
	if !asRPC(err, &re) || re.Code != "source_changed" {
		t.Fatalf("want source_changed, got %v", err)
	}
	waitLog(t, dir, "authorize-refused d1")
	if src.opens != 0 {
		t.Fatal("a part was opened without a grant")
	}
	// The session survives a refused send.
	if err := p.(plugin.PeerDispatcher).Send(context.Background(), testSend("w2", []byte("y")), &fakeSource{parts: [][]byte{[]byte("y")}}); err != nil {
		t.Fatal(err)
	}
}

func TestPeerPartOpensOnlyOnce(t *testing.T) {
	p, dir := boundPeer(t)
	src := &fakeSource{parts: [][]byte{[]byte("x")}}
	if err := p.(plugin.PeerDispatcher).Send(context.Background(), testSend("dup", []byte("x")), src); err != nil {
		t.Fatal(err)
	}
	waitLog(t, dir, "dup-refused")
}

// The handlers must refuse anything that does not match a live send.
func TestSendRequestsAreCheckedAgainstTheSend(t *testing.T) {
	p := &Proxy{}
	in := testSend("w", []byte("abc"))
	a := &activeSend{in: in, src: &fakeSource{parts: [][]byte{[]byte("abc")}}, opened: map[int]bool{}}
	p.sends = map[uint64]*activeSend{1: a}
	call := func(parent uint64, v any) *rpc.Call {
		b, _ := json.Marshal(v)
		return &rpc.Call{Parent: parent, Params: b}
	}
	good := rpc.Authorize{DispatchID: "d1", ContentHandle: tok2, ConnectionID: "c1", SessionToken: tok1}
	code := func(err error) string {
		var re *rpc.Error
		if asRPC(err, &re) {
			return re.Code
		}
		return ""
	}
	ctx := context.Background()
	wrong := good
	wrong.SessionToken = tok2
	for name, c := range map[string]*rpc.Call{
		"no such send":   call(9, good),
		"wrong token":    call(1, wrong),
		"wrong dispatch": call(1, rpc.Authorize{DispatchID: "d2", ContentHandle: tok2, ConnectionID: "c1", SessionToken: tok1}),
		"wrong handle":   call(1, rpc.Authorize{DispatchID: "d1", ContentHandle: tok1, ConnectionID: "c1", SessionToken: tok1}),
		"wrong conn":     call(1, rpc.Authorize{DispatchID: "d1", ContentHandle: tok2, ConnectionID: "c9", SessionToken: tok1}),
	} {
		if _, err := p.authorizeSend(ctx, c); code(err) != rpc.CodePermission {
			t.Errorf("%s: %v", name, err)
		}
	}
	if _, err := p.openSendPart(ctx, call(1, rpc.OpenSendPart{GrantID: testGrant, Index: 0})); code(err) != rpc.CodePermission {
		t.Errorf("open before grant: %v", err)
	}
	if _, err := p.authorizeSend(ctx, call(1, good)); err != nil {
		t.Fatal(err)
	}
	if _, err := p.authorizeSend(ctx, call(1, good)); code(err) != rpc.CodeConflict {
		t.Errorf("second grant: %v", err)
	}
	other := strings.Repeat("b", 64)
	if _, err := p.openSendPart(ctx, call(1, rpc.OpenSendPart{GrantID: other, Index: 0})); code(err) != rpc.CodePermission {
		t.Errorf("foreign grant: %v", err)
	}
	if _, err := p.openSendPart(ctx, call(1, rpc.OpenSendPart{GrantID: testGrant, Index: 5})); code(err) != rpc.CodeInvalidArgument {
		t.Errorf("index out of range: %v", err)
	}
}

func jsonMarshal(v any) ([]byte, error) { return json.Marshal(v) }

func asRPC(err error, target **rpc.Error) bool { return errors.As(err, target) }
