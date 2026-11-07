// Package engine is the heart of the daemon: it watches the local clipboard,
// keeps a session with every paired device and applies remote changes.
//
// The engine knows nothing about tailcat or the system clipboard; it talks to
// a Transport and a clip.Board, so it is tested with loopback TCP and a fake
// clipboard.
package engine

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log"
	"net"
	"os"
	"sort"
	"sync"
	"time"

	"unic/internal/clip"
	"unic/internal/store"
	"unic/internal/wire"
)

// Transport carries bytes between devices.
type Transport interface {
	// Listen accepts connections from peers.
	Listen(ctx context.Context) (net.Listener, error)
	// Dial connects to the peer with the given address.
	Dial(ctx context.Context, addr string) (net.Conn, error)
	// Addr is this device's own address, which peers dial.
	Addr() string
}

type Config struct {
	ID        string // stable device id
	Name      string
	Board     clip.Board
	Store     *store.Store
	Transport Transport
	Inbox     string        // where received files are placed
	Poll      time.Duration // clipboard poll interval; default 400ms
	Logf      func(format string, args ...any)
}

type Engine struct {
	cfg Config

	mu       sync.Mutex
	sessions map[string]*session // by peer id
	subs     map[chan Event]struct{}
	lastHash string // hash of the clipboard as we last saw it
	inboxDir string // directory holding files of the clip we last applied
	retry    map[string]context.CancelFunc
	ctx      context.Context
}

// Event is pushed to API subscribers (the GUI).
type Event struct {
	Kind string `json:"kind"` // "peer-up", "peer-down", "sent", "received", "peers"
	Peer string `json:"peer,omitempty"`
	Info string `json:"info,omitempty"`
	Time int64  `json:"time"`
}

func New(cfg Config) *Engine {
	if cfg.Poll == 0 {
		cfg.Poll = 400 * time.Millisecond
	}
	if cfg.Logf == nil {
		cfg.Logf = log.Printf
	}
	return &Engine{
		cfg:      cfg,
		sessions: map[string]*session{},
		subs:     map[chan Event]struct{}{},
		retry:    map[string]context.CancelFunc{},
	}
}

func (e *Engine) logf(f string, a ...any) { e.cfg.Logf(f, a...) }

// Run blocks until ctx is done.
func (e *Engine) Run(ctx context.Context) error {
	e.mu.Lock()
	e.ctx = ctx
	e.mu.Unlock()

	ln, err := e.cfg.Transport.Listen(ctx)
	if err != nil {
		return fmt.Errorf("listen: %w", err)
	}
	go func() { <-ctx.Done(); ln.Close() }()

	// Do not broadcast whatever is on the clipboard at startup.
	if s, err := e.cfg.Board.Read(); err == nil {
		e.lastHash = s.Hash()
	}

	go e.acceptLoop(ctx, ln)
	peers, err := e.cfg.Store.Peers()
	if err != nil {
		return err
	}
	for _, p := range peers {
		e.keepDialing(p)
	}
	e.watch(ctx)
	return ctx.Err()
}

// AddPeer pairs with the device at addr. It connects once synchronously so
// that a wrong address is reported to the user; the connection is then kept
// alive in the background.
func (e *Engine) AddPeer(ctx context.Context, addr string) (store.Peer, error) {
	e.mu.Lock()
	started := e.ctx != nil
	e.mu.Unlock()
	if !started {
		return store.Peer{}, errors.New("daemon is still starting")
	}
	if addr == e.cfg.Transport.Addr() {
		return store.Peer{}, errors.New("that is this device's own address")
	}
	conn, err := e.cfg.Transport.Dial(ctx, addr)
	if err != nil {
		return store.Peer{}, fmt.Errorf("connect: %w", err)
	}
	s, err := e.handshake(conn, true, addr)
	if err != nil {
		conn.Close()
		return store.Peer{}, err
	}
	p := store.Peer{ID: s.peerID, Name: s.peerName, Addr: addr}
	if err := e.cfg.Store.UpsertPeer(p); err != nil {
		conn.Close()
		return store.Peer{}, err
	}
	e.keepDialing(p) // reconnect after failures; a no-op while a session is up
	e.emit(Event{Kind: "peers"})
	if e.register(s) {
		go e.serve(s)
	} else {
		conn.Close() // already connected through a better session
	}
	return p, nil
}

func (e *Engine) RemovePeer(id string) (bool, error) {
	ok, err := e.cfg.Store.DeletePeer(id)
	if err != nil {
		return false, err
	}
	e.mu.Lock()
	if cancel := e.retry[id]; cancel != nil {
		cancel()
		delete(e.retry, id)
	}
	s := e.sessions[id]
	e.mu.Unlock()
	if s != nil {
		s.close()
	}
	e.emit(Event{Kind: "peers"})
	return ok, nil
}

// PeerStatus is a paired device and whether it is connected right now.
type PeerStatus struct {
	ID        string `json:"id"`
	Name      string `json:"name"`
	Connected bool   `json:"connected"`
	LastSeen  int64  `json:"last_seen,omitempty"`
}

func (e *Engine) Peers() ([]PeerStatus, error) {
	ps, err := e.cfg.Store.Peers()
	if err != nil {
		return nil, err
	}
	e.mu.Lock()
	defer e.mu.Unlock()
	out := make([]PeerStatus, 0, len(ps))
	for _, p := range ps {
		st := PeerStatus{ID: p.ID, Name: p.Name, Connected: e.sessions[p.ID] != nil}
		if !p.LastSeen.IsZero() {
			st.LastSeen = p.LastSeen.Unix()
		}
		out = append(out, st)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

func (e *Engine) Self() (id, name, addr string) {
	return e.cfg.ID, e.cfg.Name, e.cfg.Transport.Addr()
}

// Subscribe returns a channel of events and a function that stops it.
func (e *Engine) Subscribe() (<-chan Event, func()) {
	ch := make(chan Event, 32)
	e.mu.Lock()
	e.subs[ch] = struct{}{}
	e.mu.Unlock()
	return ch, func() {
		e.mu.Lock()
		delete(e.subs, ch)
		e.mu.Unlock()
	}
}

func (e *Engine) emit(ev Event) {
	ev.Time = time.Now().Unix()
	e.mu.Lock()
	defer e.mu.Unlock()
	for ch := range e.subs {
		select {
		case ch <- ev:
		default: // a slow subscriber misses events instead of stalling the engine
		}
	}
}

// ---- clipboard watching ----

func (e *Engine) watch(ctx context.Context) {
	t := time.NewTicker(e.cfg.Poll)
	defer t.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-t.C:
		}
		s, err := e.cfg.Board.Read()
		if err != nil {
			continue // transient: the owner changed mid-read, or the board is locked
		}
		h := s.Hash()
		e.mu.Lock()
		changed := h != e.lastHash
		e.lastHash = h
		e.mu.Unlock()
		if !changed || h == "" {
			continue
		}
		e.broadcast(s)
	}
}

// SetLocal writes s to the local clipboard on behalf of the user (CLI/GUI)
// and syncs it like any other change.
func (e *Engine) SetLocal(s clip.Snapshot) error {
	if err := e.cfg.Board.Write(s); err != nil {
		return err
	}
	// The watcher will notice the write and broadcast it.
	return nil
}

func (e *Engine) Current() (clip.Snapshot, error) { return e.cfg.Board.Read() }

func (e *Engine) broadcast(s clip.Snapshot) {
	files, err := statFiles(s.Files)
	if err != nil {
		// A folder (or a vanished file) is on the clipboard. Its name as text
		// would be a misleading clip, so sync nothing.
		e.logf("not syncing clipboard files: %v", err)
		return
	}
	s.Files = files
	if s.Empty() {
		return
	}
	id := wire.NewID()
	ts := time.Now().UnixMilli()
	e.mu.Lock()
	targets := make([]*session, 0, len(e.sessions))
	for _, ss := range e.sessions {
		targets = append(targets, ss)
	}
	e.mu.Unlock()
	for _, ss := range targets {
		ss.enqueue(outClip{id: id, ts: ts, snap: s})
	}
	if len(targets) > 0 {
		e.emit(Event{Kind: "sent", Info: s.Summary()})
	}
}

// statFiles keeps regular files only: directories are not synced yet.
func statFiles(in []clip.File) ([]clip.File, error) {
	var out []clip.File
	for _, f := range in {
		st, err := os.Stat(f.Path)
		if err != nil {
			return nil, err
		}
		if !st.Mode().IsRegular() {
			return nil, fmt.Errorf("%s is not a regular file", f.Path)
		}
		if st.Size() > wire.MaxFileBytes {
			return nil, fmt.Errorf("%s is too large to sync", f.Path)
		}
		out = append(out, clip.File{Name: wire.SafeName(f.Name), Path: f.Path, Size: st.Size()})
	}
	return out, nil
}

// apply installs a remote clipboard locally.
func (e *Engine) apply(from string, s clip.Snapshot, dir string) {
	h := s.Hash()
	e.mu.Lock()
	if h == e.lastHash {
		e.mu.Unlock()
		if dir != "" {
			os.RemoveAll(dir)
		}
		return
	}
	e.lastHash = h // so the watcher does not echo it back
	old := e.inboxDir
	e.inboxDir = dir
	e.mu.Unlock()

	if err := e.cfg.Board.Write(s); err != nil {
		e.logf("write clipboard from %s: %v", from, err)
		e.mu.Lock()
		e.lastHash = ""
		e.mu.Unlock()
		return
	}
	// The system may add representations of its own to what we wrote, which
	// would change the hash and bounce the clip back to its sender. Take the
	// hash of what is really on the clipboard now as the baseline.
	if back, err := e.cfg.Board.Read(); err == nil {
		e.mu.Lock()
		e.lastHash = back.Hash()
		e.mu.Unlock()
	}
	// Received files stay while they are on the clipboard; the previous
	// batch can go now.
	if old != "" && old != dir {
		os.RemoveAll(old)
	}
	e.emit(Event{Kind: "received", Peer: from, Info: s.Summary()})
}

// ---- sessions ----

type outClip struct {
	id   string
	ts   int64
	snap clip.Snapshot
}

type session struct {
	e        *Engine
	conn     net.Conn
	w        *wire.Conn
	peerID   string
	peerName string
	peerAddr string
	outgoing bool
	out      chan outClip
	done     chan struct{}
	once     sync.Once
}

func (s *session) close() { s.once.Do(func() { close(s.done); s.conn.Close() }) }

// enqueue keeps only the newest pending clip: a slow link should deliver the
// latest clipboard, not a backlog of stale ones.
func (s *session) enqueue(c outClip) {
	for {
		select {
		case s.out <- c:
			return
		case <-s.done:
			return
		default:
			select {
			case <-s.out:
			default:
			}
		}
	}
}

const handshakeTimeout = 15 * time.Second

func (e *Engine) handshake(conn net.Conn, outgoing bool, dialed string) (*session, error) {
	_ = conn.SetDeadline(time.Now().Add(handshakeTimeout))
	defer conn.SetDeadline(time.Time{})
	w := wire.NewConn(conn)
	hello := wire.Hello{T: "hello", Version: wire.Version, ID: e.cfg.ID, Name: e.cfg.Name, Addr: e.cfg.Transport.Addr()}
	werr := make(chan error, 1)
	go func() { werr <- w.WriteJSON(hello) }()
	typ, raw, err := w.ReadRaw()
	if err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if err := <-werr; err != nil {
		return nil, fmt.Errorf("handshake: %w", err)
	}
	if typ != "hello" {
		return nil, fmt.Errorf("handshake: unexpected %q message", typ)
	}
	var h wire.Hello
	if err := json.Unmarshal(raw, &h); err != nil {
		return nil, err
	}
	if h.ID == "" || h.ID == e.cfg.ID {
		return nil, errors.New("handshake: bad device id")
	}
	if h.Version != wire.Version {
		return nil, fmt.Errorf("handshake: peer speaks protocol %d, we speak %d", h.Version, wire.Version)
	}
	s := &session{e: e, conn: conn, w: w, peerID: h.ID, peerName: h.Name, peerAddr: h.Addr,
		outgoing: outgoing, out: make(chan outClip, 1), done: make(chan struct{})}
	if outgoing && dialed != "" && s.peerAddr == "" {
		s.peerAddr = dialed
	}
	return s, nil
}

func (e *Engine) acceptLoop(ctx context.Context, ln net.Listener) {
	for {
		conn, err := ln.Accept()
		if err != nil {
			if ctx.Err() == nil {
				e.logf("accept: %v", err)
			}
			return
		}
		go func() {
			s, err := e.handshake(conn, false, "")
			if err != nil {
				e.logf("incoming connection rejected: %v", err)
				conn.Close()
				return
			}
			// Whoever holds our address (a secret) may pair with us. Remember
			// the peer so that we can reconnect after a restart.
			if s.peerAddr != "" {
				known := false
				if ps, err := e.cfg.Store.Peers(); err == nil {
					for _, p := range ps {
						known = known || p.ID == s.peerID
					}
				}
				if err := e.cfg.Store.UpsertPeer(store.Peer{ID: s.peerID, Name: s.peerName, Addr: s.peerAddr}); err != nil {
					e.logf("remember peer: %v", err)
				} else if !known {
					e.logf("paired with %s (%s)", s.peerName, s.peerID)
					e.emit(Event{Kind: "peers"})
				}
			}
			if !e.register(s) {
				conn.Close()
				return
			}
			e.serve(s)
		}()
	}
}

// register adds s unless a better session to the same peer exists. Both
// devices apply the same rule, so they agree on which of two simultaneous
// connections survives: the one dialed by the device with the smaller id.
func (e *Engine) register(s *session) bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	if old := e.sessions[s.peerID]; old != nil {
		select {
		case <-old.done:
		default:
			if dialer(e.cfg.ID, old) <= dialer(e.cfg.ID, s) {
				return false
			}
			old.close()
		}
	}
	e.sessions[s.peerID] = s
	return true
}

func dialer(self string, s *session) string {
	if s.outgoing {
		return self
	}
	return s.peerID
}

// serve runs a registered session until it ends.
func (e *Engine) serve(s *session) {
	e.logf("connected to %s", s.peerName)
	_ = e.cfg.Store.TouchPeer(s.peerID)
	e.emit(Event{Kind: "peer-up", Peer: s.peerName})
	go s.sendLoop()
	err := s.recvLoop()
	s.close()
	e.mu.Lock()
	if e.sessions[s.peerID] == s {
		delete(e.sessions, s.peerID)
	}
	e.mu.Unlock()
	_ = e.cfg.Store.TouchPeer(s.peerID)
	if err != nil && !errors.Is(err, io.EOF) && !errors.Is(err, net.ErrClosed) {
		e.logf("session with %s ended: %v", s.peerName, err)
	}
	e.emit(Event{Kind: "peer-down", Peer: s.peerName})
}

func (s *session) sendLoop() {
	for {
		select {
		case <-s.done:
			return
		case c := <-s.out:
			if err := s.w.SendClip(c.id, s.e.cfg.ID, c.ts, c.snap); err != nil {
				s.e.logf("send to %s: %v", s.peerName, err)
				s.close()
				return
			}
		}
	}
}

func (s *session) recvLoop() error {
	for {
		typ, raw, err := s.w.ReadRaw()
		if err != nil {
			return err
		}
		switch typ {
		case "clip":
			var m wire.Clip
			if err := json.Unmarshal(raw, &m); err != nil {
				return err
			}
			snap, dir, err := s.w.ReceiveClip(&m, s.e.cfg.Inbox)
			if err != nil {
				return err
			}
			if snap.Hash() != m.Hash {
				// Corrupt or tampered: the data is already consumed, so the
				// stream is still in sync; just drop it.
				if dir != "" {
					os.RemoveAll(dir)
				}
				s.e.logf("dropped clip from %s: hash mismatch", s.peerName)
				continue
			}
			s.e.apply(s.peerName, snap, dir)
		default:
			// Unknown control messages are ignored for forward compatibility.
		}
	}
}

// keepDialing maintains an outgoing connection to p, with backoff.
func (e *Engine) keepDialing(p store.Peer) {
	e.mu.Lock()
	if e.retry[p.ID] != nil || e.ctx == nil {
		e.mu.Unlock()
		return
	}
	ctx, cancel := context.WithCancel(e.ctx)
	e.retry[p.ID] = cancel
	e.mu.Unlock()

	go func() {
		backoff := time.Second
		for ctx.Err() == nil {
			e.mu.Lock()
			up := e.sessions[p.ID] != nil
			e.mu.Unlock()
			if up {
				if !sleep(ctx, 5*time.Second) {
					return
				}
				backoff = time.Second
				continue
			}
			dctx, dcancel := context.WithTimeout(ctx, 30*time.Second)
			conn, err := e.cfg.Transport.Dial(dctx, p.Addr)
			dcancel()
			var s *session
			if err == nil {
				s, err = e.handshake(conn, true, p.Addr)
				if err != nil {
					conn.Close()
				}
			}
			if err == nil && s.peerID != p.ID {
				conn.Close()
				err = errors.New("address belongs to a different device")
			}
			if err != nil {
				if ctx.Err() == nil {
					e.logf("cannot reach %s: %v (retry in %v)", p.Name, err, backoff)
				}
				if !sleep(ctx, backoff) {
					return
				}
				if backoff < time.Minute {
					backoff *= 2
				}
				continue
			}
			backoff = time.Second
			if !e.register(s) {
				conn.Close()
				continue
			}
			e.serve(s)
		}
	}()
}

func sleep(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}
