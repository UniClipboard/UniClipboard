// Package loopback provides an isolated acceptance fixture, not a production transport.
package loopback

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/sha256"
	_ "embed"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Peer is a peer_sessions fixture over loopback TCP. The engine
// tests run it as a real child process, so Engine, RPC framing, identity
// handshake, pairing, send and receive are exercised together; only the
// transport is simplified (length-prefixed frames, no encryption).
type Peer struct {
	traceMu     sync.Mutex
	traceEvents []map[string]any
	host        plugin.PeerHost
	ctx         context.Context
	stop        context.CancelFunc

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
	nc            net.Conn
	conn          string
	token         string
	trustRevision rpc.U64
	init          string // device ID of the side that dialed
	wmu           sync.Mutex
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

//go:embed settings.schema.json
var settingsSchema []byte

func (p *Peer) Spec() plugin.Spec {
	digest := sha256.Sum256(settingsSchema)
	return plugin.Spec{ID: "e2e.tcp", SettingsSHA256: hex.EncodeToString(digest[:]), Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}
func (p *Peer) SetPeerHost(h plugin.PeerHost) { p.host = h }
func (p *Peer) Init(_ context.Context, _ plugin.Host, cfg plugin.Config) error {
	for key := range cfg {
		if key != "marker" && key != "credential" {
			return plugin.ErrInvalidConfig
		}
	}
	marker := "loopback"
	if v, ok := cfg["marker"]; ok {
		var valid bool
		marker, valid = v.(string)
		if !valid {
			return plugin.ErrInvalidConfig
		}
	}
	if err := os.WriteFile("marker", []byte(marker), 0600); err != nil {
		return err
	}
	p.peers, p.conns, p.dialing = map[string]rpc.StatePeer{}, map[string]*tcpSess{}, map[string]bool{}
	p.ctx, p.stop = context.WithCancel(context.Background())
	return nil
}
func (p *Peer) Start(context.Context) error {
	address := "127.0.0.1:0"
	if saved, err := os.ReadFile("endpoint"); err == nil {
		address = string(saved)
		host, port, err := net.SplitHostPort(address)
		if err != nil || host != "127.0.0.1" || port == "0" {
			return plugin.ErrInvalidConfig
		}
	} else if !os.IsNotExist(err) {
		return err
	}
	ln, err := net.Listen("tcp", address)
	if err != nil {
		return err
	}
	if err := os.WriteFile("endpoint", []byte(ln.Addr().String()), 0600); err != nil {
		_ = ln.Close()
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
func (p *Peer) Close(context.Context) error {
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
func (p *Peer) Bind(_ context.Context, b rpc.Bind) error {
	p.mu.Lock()
	p.self, p.name = b.SelfID, b.SelfName
	p.mu.Unlock()
	return nil
}

// ApplyState returns only after sessions to removed peers are closed.
func (p *Peer) ApplyState(_ context.Context, s rpc.SyncState) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	p.trace("apply-state", fmt.Sprintf("peers=%d revision=%d sync=%v", len(s.Peers), s.Revision, s.SyncEnabled), nil)
	p.introduce = s.Introduce
	p.peers = map[string]rpc.StatePeer{}
	for _, pe := range s.Peers {
		p.peers[pe.ID] = pe
	}
	for id, c := range p.conns {
		if peer, ok := p.peers[id]; !ok || peer.TrustRevision != c.trustRevision {
			_ = c.nc.Close()
		}
	}
	go p.introduceAll()
	return nil
}

// introduceAll tells every connected peer about the other peers it knows.
func (p *Peer) introduceAll() {
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

func (p *Peer) PairingAddress(context.Context, string) (string, error) {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.ln.Addr().String(), nil
}

func (p *Peer) Connect(_ context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
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
func (p *Peer) handshake(nc net.Conn, outgoing bool, attempt *string) (peerID string, remoteAllow bool, err error) {
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
		h, err := p.host.IdentityHello(ctx, rpc.IdentityHello{ConnectionID: conn, Direction: "outgoing", AttemptID: attempt, ContextBinding: base64.RawURLEncoding.EncodeToString([]byte(conn))})
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
		h, err := p.host.IdentityHello(ctx, rpc.IdentityHello{ConnectionID: conn, Direction: "incoming", ContextBinding: base64.RawURLEncoding.EncodeToString([]byte(conn))})
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
	p.trace("check-peer", fmt.Sprintf("%s revision=%d", dec.Decision, dec.TrustRevision), nil)
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
	s := &tcpSess{nc: nc, conn: conn, token: *dec.SessionToken, trustRevision: dec.TrustRevision, init: dec.PeerID}
	p.mu.Lock()
	if outgoing {
		s.init = p.self
	}
	// Two connections to one peer: the one dialed by the smaller device ID
	// wins on both sides, as the Engine expects.
	if old := p.conns[dec.PeerID]; old != nil {
		if old.init <= s.init {
			p.mu.Unlock()
			return dec.PeerID, true, nil // the new one loses; the deferred close ends it
		}
		_ = old.nc.Close()
	}
	p.conns[dec.PeerID] = s
	p.mu.Unlock()
	if err = p.host.PeerEvent(ctx, rpc.PeerEvent{ConnectionID: conn, SessionToken: s.token, Sequence: 1, State: "connected", RemoteAccepted: true}); err != nil {
		p.mu.Lock()
		if p.conns[dec.PeerID] == s {
			delete(p.conns, dec.PeerID)
		}
		p.mu.Unlock()
		return dec.PeerID, true, err
	}
	p.trace("connected", "", nil)
	ok = true
	_ = nc.SetDeadline(time.Time{})
	go p.serve(dec.PeerID, s)
	go p.introduceAll()
	return dec.PeerID, true, nil
}

// serve reads clips from a session until it closes.
func (p *Peer) serve(peer string, s *tcpSess) {
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
		p.trace("receive-open", g.Decision, err)
		if err != nil || g.Decision != rpc.DecisionAccept {
			continue
		}
		_, err = p.host.Receive(p.ctx, *g.ReceiveID, bodies)
		p.trace("receive-complete", "", err)
	}
}

func (p *Peer) Send(ctx context.Context, s rpc.Send, h plugin.SendHost) (rpc.SendResult, error) {
	if _, err := os.Stat("crash-send"); err == nil {
		_ = os.Remove("crash-send")
		os.Exit(3)
	}
	if _, err := os.Stat("fail-send"); err == nil {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	p.mu.Lock()
	sess := p.conns[s.PeerID]
	p.mu.Unlock()
	if sess == nil {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeNotFound)
	}
	if sess.conn != s.ConnectionID {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeConflict)
	}
	g, err := h.Authorize(ctx)
	p.trace("authorize-send", "", err)
	if err != nil {
		return rpc.SendResult{}, err
	}
	sess.wmu.Lock()
	defer sess.wmu.Unlock()
	if err := sendMsg(sess.nc, tcpMsg{Clip: &s.Clip}); err != nil {
		return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
	}
	for i := range s.Clip.Parts {
		r, err := h.OpenPart(ctx, g.GrantID, i)
		if err != nil {
			return rpc.SendResult{}, err
		}
		b, err := io.ReadAll(r)
		_ = r.Close()
		if err != nil {
			return rpc.SendResult{}, err
		}
		if err := writeFrame(sess.nc, b); err != nil {
			return rpc.SendResult{}, rpc.NewError(rpc.CodeUnavailable)
		}
	}
	return rpc.SendResult{State: rpc.SendSent, ReceiptStage: rpc.ReceiptLocalWritten}, nil
}

// New returns a loopback-only, unencrypted peer backend for process acceptance.
func New() plugin.Plugin { return &Peer{} }

// trace retains bounded, secret-free fixture diagnostics in its private directory.
func (p *Peer) trace(stage, decision string, err error) {
	code := ""
	if err != nil {
		var rpcErr *rpc.Error
		if errors.As(err, &rpcErr) {
			code = rpcErr.Code
		} else {
			code = "internal"
		}
	}
	p.traceMu.Lock()
	defer p.traceMu.Unlock()
	p.traceEvents = append(p.traceEvents, map[string]any{"stage": stage, "decision": decision, "error": code, "at_ms": time.Now().UnixMilli()})
	if len(p.traceEvents) > 64 {
		p.traceEvents = p.traceEvents[len(p.traceEvents)-64:]
	}
	raw, _ := json.Marshal(p.traceEvents)
	_ = os.WriteFile("fixture-trace.json", raw, 0600)
}
