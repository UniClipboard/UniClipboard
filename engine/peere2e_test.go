package engine

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"io"
	"net"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// tcpPeer is a complete peer_sessions backend over loopback TCP. The engine
// tests run it as a real child process, so Engine, RPC framing, identity
// handshake, pairing, send and receive are exercised together; only the
// transport is simplified (length-prefixed frames, no encryption).
type tcpPeer struct {
	host plugin.PeerHost
	ctx  context.Context
	stop context.CancelFunc

	mu        sync.Mutex
	self      string
	name      string
	ln        net.Listener
	peers     map[string]rpc.StatePeer
	conns     map[string]*tcpSess
	dialing   map[string]bool
	introduce bool
}

type tcpSess struct {
	nc    net.Conn
	conn  string
	token string
	wmu   sync.Mutex
}

type tcpMsg struct {
	Conn  string          `json:"conn,omitempty"`
	Hello string          `json:"hello,omitempty"`
	Proof string          `json:"proof,omitempty"`
	Pair  *string         `json:"pair,omitempty"`
	Name  string          `json:"name,omitempty"`
	Addr  string          `json:"addr,omitempty"`
	Allow bool            `json:"allow,omitempty"`
	Clip  *rpc.Clip       `json:"clip,omitempty"`
	Cands []rpc.Candidate `json:"cands,omitempty"`
	Intro bool            `json:"intro,omitempty"`
}

func (p *tcpPeer) Spec() plugin.Spec {
	return plugin.Spec{ID: "e2e.tcp", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}
func (p *tcpPeer) SetPeerHost(h plugin.PeerHost) { p.host = h }
func (p *tcpPeer) Init(context.Context, plugin.Host, plugin.Config) error {
	p.peers, p.conns, p.dialing = map[string]rpc.StatePeer{}, map[string]*tcpSess{}, map[string]bool{}
	p.ctx, p.stop = context.WithCancel(context.Background())
	return nil
}
func (p *tcpPeer) Start(context.Context) error {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return err
	}
	p.mu.Lock()
	p.ln = ln
	p.mu.Unlock()
	go func() {
		for {
			nc, err := ln.Accept()
			if err != nil {
				return
			}
			go func() { _, _, _ = p.handshake(nc, false, nil) }()
		}
	}()
	go func() {
		for p.ctx.Err() == nil {
			time.Sleep(100 * time.Millisecond)
			p.mu.Lock()
			for id, pe := range p.peers {
				if pe.Address != "" && p.self < id && p.conns[id] == nil && !p.dialing[id] {
					p.dialing[id] = true
					go func(id, addr string) {
						defer func() { p.mu.Lock(); delete(p.dialing, id); p.mu.Unlock() }()
						if nc, err := net.DialTimeout("tcp", addr, 2*time.Second); err == nil {
							_, _, _ = p.handshake(nc, true, nil)
						}
					}(id, pe.Address)
				}
			}
			p.mu.Unlock()
		}
	}()
	return nil
}
func (p *tcpPeer) Close(context.Context) error {
	p.stop()
	p.mu.Lock()
	defer p.mu.Unlock()
	if p.ln != nil {
		_ = p.ln.Close()
	}
	for _, s := range p.conns {
		_ = s.nc.Close()
	}
	return nil
}
func (p *tcpPeer) Bind(_ context.Context, b rpc.Bind) error {
	p.mu.Lock()
	p.self, p.name = b.SelfID, b.SelfName
	p.mu.Unlock()
	return nil
}

// ApplyState returns only after sessions to removed peers are closed.
func (p *tcpPeer) ApplyState(_ context.Context, s rpc.SyncState) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.introduce = s.Introduce
	p.peers = map[string]rpc.StatePeer{}
	for _, pe := range s.Peers {
		p.peers[pe.ID] = pe
	}
	for id, c := range p.conns {
		if _, ok := p.peers[id]; !ok {
			_ = c.nc.Close()
		}
	}
	go p.introduceAll()
	return nil
}

// introduceAll tells every connected peer about the other peers it knows.
func (p *tcpPeer) introduceAll() {
	p.mu.Lock()
	if !p.introduce {
		p.mu.Unlock()
		return
	}
	type target struct {
		s     *tcpSess
		cands []rpc.Candidate
	}
	var ts []target
	for id, s := range p.conns {
		cs := []rpc.Candidate{}
		for oid, o := range p.peers {
			if oid != id && o.Address != "" {
				cs = append(cs, rpc.Candidate{ID: oid, Name: o.Name, Address: o.Address})
			}
		}
		ts = append(ts, target{s, cs})
	}
	p.mu.Unlock()
	for _, t := range ts {
		t.s.wmu.Lock()
		_ = sendMsg(t.s.nc, tcpMsg{Intro: true, Cands: t.cands})
		t.s.wmu.Unlock()
	}
}

func (p *tcpPeer) PairingAddress(context.Context, string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.ln.Addr().String(), nil
}

func (p *tcpPeer) Connect(_ context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	nc, err := net.DialTimeout("tcp", c.Address, 5*time.Second)
	if err != nil {
		return rpc.ConnectResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	id, remote, err := p.handshake(nc, true, &c.AttemptID)
	if err != nil {
		return rpc.ConnectResult{}, err
	}
	return rpc.ConnectResult{PeerID: id, AwaitingRemoteApproval: !remote}, nil
}

func writeFrame(w io.Writer, b []byte) error {
	var h [4]byte
	binary.BigEndian.PutUint32(h[:], uint32(len(b)))
	_, err := w.Write(append(h[:], b...))
	return err
}
func readFrame(r io.Reader) ([]byte, error) {
	var h [4]byte
	if _, err := io.ReadFull(r, h[:]); err != nil {
		return nil, err
	}
	n := binary.BigEndian.Uint32(h[:])
	if n > 1<<20 {
		return nil, rpc.NewError(rpc.CodeLimitExceeded)
	}
	b := make([]byte, n)
	_, err := io.ReadFull(r, b)
	return b, err
}
func sendMsg(w io.Writer, m tcpMsg) error { b, _ := json.Marshal(m); return writeFrame(w, b) }
func recvMsg(r io.Reader) (m tcpMsg, err error) {
	b, err := readFrame(r)
	if err == nil {
		err = json.Unmarshal(b, &m)
	}
	return
}

// handshake runs the identity exchange on nc. When both sides allow, the
// connection becomes a session; otherwise it is closed.
func (p *tcpPeer) handshake(nc net.Conn, outgoing bool, attempt *string) (peerID string, remoteAllow bool, err error) {
	ok := false
	defer func() {
		if !ok {
			_ = nc.Close()
		}
	}()
	_ = nc.SetDeadline(time.Now().Add(10 * time.Second))
	ctx, cancel := context.WithTimeout(p.ctx, 10*time.Second)
	defer cancel()
	p.mu.Lock()
	name, addr := p.name, p.ln.Addr().String()
	p.mu.Unlock()

	var conn string
	var prove rpc.IdentityProveResult
	var theirs tcpMsg
	if outgoing {
		var raw [16]byte
		_, _ = rand.Read(raw[:])
		conn = hex.EncodeToString(raw[:])
		h, err := p.host.IdentityHello(ctx, rpc.IdentityHello{ConnectionID: conn, Direction: "outgoing", AttemptID: attempt, ContextBinding: b64.EncodeToString([]byte(conn))})
		if err != nil {
			return "", false, err
		}
		if err = sendMsg(nc, tcpMsg{Conn: conn, Hello: h.Hello}); err != nil {
			return "", false, err
		}
		m, err := recvMsg(nc)
		if err != nil {
			return "", false, err
		}
		if prove, err = p.host.IdentityProve(ctx, rpc.IdentityProve{ConnectionID: conn, PeerHello: m.Hello}); err != nil {
			return "", false, err
		}
		if err = sendMsg(nc, tcpMsg{Proof: prove.Proof, Pair: prove.PairProof, Name: name, Addr: addr}); err != nil {
			return "", false, err
		}
		if theirs, err = recvMsg(nc); err != nil {
			return "", false, err
		}
	} else {
		m, err := recvMsg(nc)
		if err != nil {
			return "", false, err
		}
		conn = m.Conn
		h, err := p.host.IdentityHello(ctx, rpc.IdentityHello{ConnectionID: conn, Direction: "incoming", ContextBinding: b64.EncodeToString([]byte(conn))})
		if err != nil {
			return "", false, err
		}
		if err = sendMsg(nc, tcpMsg{Hello: h.Hello}); err != nil {
			return "", false, err
		}
		if prove, err = p.host.IdentityProve(ctx, rpc.IdentityProve{ConnectionID: conn, PeerHello: m.Hello}); err != nil {
			return "", false, err
		}
		if theirs, err = recvMsg(nc); err != nil {
			return "", false, err
		}
		if err = sendMsg(nc, tcpMsg{Proof: prove.Proof, Name: name, Addr: addr}); err != nil {
			return "", false, err
		}
	}
	dir := "incoming"
	if outgoing {
		dir = "outgoing"
	}
	dec, err := p.host.CheckPeer(ctx, rpc.CheckPeer{ConnectionID: conn, Direction: dir, AttemptID: attempt,
		Hello: rpc.PeerWire{WireVersion: rpc.WireVersion, Name: theirs.Name, Address: theirs.Addr}, PeerProof: theirs.Proof, PeerPairProof: theirs.Pair})
	if err != nil {
		return "", false, err
	}
	local := dec.Decision == "allow"
	if err = sendMsg(nc, tcpMsg{Allow: local}); err != nil {
		return dec.PeerID, false, err
	}
	m, err := recvMsg(nc)
	if err != nil {
		return dec.PeerID, false, err
	}
	if !local || !m.Allow {
		return dec.PeerID, m.Allow, nil
	}
	s := &tcpSess{nc: nc, conn: conn, token: *dec.SessionToken}
	p.mu.Lock()
	if old := p.conns[dec.PeerID]; old != nil {
		_ = old.nc.Close()
	}
	p.conns[dec.PeerID] = s
	p.mu.Unlock()
	if err = p.host.PeerEvent(ctx, rpc.PeerEvent{ConnectionID: conn, SessionToken: s.token, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
		_ = nc.Close()
		return dec.PeerID, true, err
	}
	ok = true
	_ = nc.SetDeadline(time.Time{})
	go p.serve(dec.PeerID, s)
	go p.introduceAll()
	return dec.PeerID, true, nil
}

// serve reads clips from a session until it closes.
func (p *tcpPeer) serve(peer string, s *tcpSess) {
	defer func() {
		_ = s.nc.Close()
		p.mu.Lock()
		if p.conns[peer] == s {
			delete(p.conns, peer)
		}
		p.mu.Unlock()
		c, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		_ = p.host.PeerEvent(c, rpc.PeerEvent{ConnectionID: s.conn, SessionToken: s.token, Sequence: 2, State: "disconnected"})
	}()
	for {
		m, err := recvMsg(s.nc)
		if err != nil {
			return
		}
		if m.Intro {
			_ = p.host.Introduce(p.ctx, rpc.Introduce{ConnectionID: s.conn, SessionToken: s.token, Candidates: m.Cands})
			continue
		}
		if m.Clip == nil {
			return
		}
		bodies := make([]plugin.ReceiveBody, len(m.Clip.Parts))
		for i := range bodies {
			b, err := readFrame(s.nc)
			if err != nil {
				return
			}
			bodies[i] = plugin.ReceiveBody{Size: uint64(len(b)), R: bytes.NewReader(b)}
		}
		g, err := p.host.ReceiveOpen(p.ctx, rpc.ReceiveOpen{ConnectionID: s.conn, SessionToken: s.token, Clip: *m.Clip})
		if err != nil || g.Decision != rpc.DecisionAccept {
			continue
		}
		_, _ = p.host.Receive(p.ctx, *g.ReceiveID, bodies)
	}
}

func (p *tcpPeer) Send(ctx context.Context, s rpc.Send, h plugin.SendHost) error {
	p.mu.Lock()
	sess := p.conns[s.PeerID]
	p.mu.Unlock()
	if sess == nil || sess.conn != s.ConnectionID {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	g, err := h.Authorize(ctx)
	if err != nil {
		return err
	}
	sess.wmu.Lock()
	defer sess.wmu.Unlock()
	if err := sendMsg(sess.nc, tcpMsg{Clip: &s.Clip}); err != nil {
		return rpc.NewError(rpc.CodeUnavailable)
	}
	for i := range s.Clip.Parts {
		r, err := h.OpenPart(ctx, g.GrantID, i)
		if err != nil {
			return err
		}
		b, err := io.ReadAll(r)
		_ = r.Close()
		if err != nil {
			return err
		}
		if err := writeFrame(sess.nc, b); err != nil {
			return rpc.NewError(rpc.CodeUnavailable)
		}
	}
	return nil
}

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
		var id OperationID
		eventually(t, "target connected", func() bool {
			h := importText(t, from.e, text)
			var err error
			if id, err = submit(from.e, h); err != nil {
				return false
			}
			op := waitSend(t, from.e, id)
			return len(op.Targets) == 1 && op.Targets[0].State == TargetSent
		})
		eventually(t, "received", func() bool {
			for _, r := range received(to.e) {
				if string(r.PeerID) == from.e.ident.DeviceID() {
					return true
				}
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

	eventually(t, "A–C sync", func() bool {
		id, err := submit(nc.e, importText(t, nc.e, "from c"))
		if err != nil {
			return false
		}
		o := waitSend(t, nc.e, id)
		sent := false
		for _, tg := range o.Targets {
			if tg.State == TargetSent && string(tg.PeerID) == idA {
				sent = true
			}
		}
		return sent
	})
	eventually(t, "A received from C", func() bool {
		for _, r := range received(na.e) {
			if string(r.PeerID) == idC {
				return true
			}
		}
		return false
	})
}
