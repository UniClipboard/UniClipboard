package session

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/wire"
	"github.com/uniclipboard/unic/plugin/rpc"
)

const (
	defaultProbeTimeout   = 4 * time.Second
	discoProbeTimeout     = 3 * time.Second
	recentActivityWindow  = 5 * time.Second
	connectingGraceWindow = 35 * time.Second
	maxConnectingAttempts = 2
	maxProbeConcurrency   = 8
	maxPingIDBytes        = 64
)

type dialStatus struct {
	owner             *dialer
	inFlight          bool
	attempts          int
	activeUntil       time.Time
	visibleConnecting bool
}

// dialConnectingLocked reports whether a disconnected peer should be displayed
// as "connecting". Only initial startup, unexpected session drops, and explicit
// manual refreshes open a visible connecting window; background retries and
// network-change resets preserve an already-settled disconnected state.
func (b *Backend) dialConnectingLocked(peerID string, now time.Time) bool {
	if b.run == nil || b.connectedLocked(peerID) {
		return false
	}
	ds := b.dialing[peerID]
	return ds.visibleConnecting && ds.attempts < maxConnectingAttempts && now.Before(ds.activeUntil)
}

type peerLinkCache struct {
	path          string
	latencyMs     int64
	lastCheckedMs int64
}

func (b *Backend) probeBudget() time.Duration {
	if b.cfg.ProbeTimeout > 0 {
		return b.cfg.ProbeTimeout
	}
	return defaultProbeTimeout
}

func (b *Backend) notifyLinkChanged(peerID string) {
	b.mu.Lock()
	cb := b.onLinkChange
	b.mu.Unlock()
	if cb == nil || peerID == "" {
		return
	}
	defer func() { _ = recover() }()
	cb(peerID)
}

// SetLinkStatusChanged registers an optional in-process callback invoked when a
// peer's non-session link state (such as connecting/probed metadata) changes.
func (b *Backend) SetLinkStatusChanged(fn func(peerID string)) {
	b.mu.Lock()
	b.onLinkChange = fn
	b.mu.Unlock()
}

func (s *session) acquireWrite(ctx context.Context) bool {
	if s.wlock == nil {
		return !s.isDone() && ctx.Err() == nil
	}
	select {
	case s.wlock <- struct{}{}:
		return true
	case <-s.done:
		return false
	case <-ctx.Done():
		return false
	}
}

func (s *session) releaseWrite() {
	if s.wlock != nil {
		<-s.wlock
	}
}

func (s *session) markRx() {
	s.lastRxUnixNs.Store(time.Now().UnixNano())
}

func (s *session) markTx() {
	s.lastTxUnixNs.Store(time.Now().UnixNano())
}

func (s *session) hasRecentBodyProgress(window time.Duration) bool {
	if !s.bodyActive.Load() {
		return false
	}
	cutoff := time.Now().Add(-window).UnixNano()
	return s.lastRxUnixNs.Load() >= cutoff || s.lastTxUnixNs.Load() >= cutoff
}

func (s *session) hasRxSince(since time.Time) bool {
	return s.lastRxUnixNs.Load() >= since.UnixNano()
}

func (s *session) handlePing(raw []byte) {
	var msg wire.Ping
	if err := json.Unmarshal(raw, &msg); err != nil || msg.ID == "" || len(msg.ID) > maxPingIDBytes {
		return
	}
	// At most one pong write queued per session to avoid goroutine build-up.
	select {
	case s.pongSem <- struct{}{}:
	default:
		return
	}
	s.pongWG.Add(1)
	go func(id string) {
		defer s.pongWG.Done()
		defer func() { <-s.pongSem }()
		ctx, cancel := context.WithTimeout(s.r.ctx, eventTimeout)
		defer cancel()
		if !s.acquireWrite(ctx) {
			return
		}
		defer s.releaseWrite()
		_ = s.conn.SetWriteDeadline(time.Now().Add(eventTimeout))
		err := s.w.WriteJSON(wire.Pong{T: "pong", ID: id})
		_ = s.conn.SetWriteDeadline(time.Time{})
		if err != nil {
			s.closeWith(reason(err))
			return
		}
		s.markTx()
	}(msg.ID)
}

func (s *session) handlePong(raw []byte) {
	var msg wire.Pong
	if err := json.Unmarshal(raw, &msg); err != nil || msg.ID == "" || len(msg.ID) > maxPingIDBytes {
		return
	}
	s.probeMu.Lock()
	ch := s.probes[msg.ID]
	if ch != nil {
		delete(s.probes, msg.ID)
		close(ch)
	}
	s.probeMu.Unlock()
}

// ping verifies that the remote peer responds over this authenticated session.
// If the probe times out or fails on an idle connection, the stale session is
// closed immediately so the dialer can establish a fresh one. Active bulk body
// transfers with recent I/O progress are never falsely closed by a probe.
func (s *session) ping(ctx context.Context) (time.Duration, error) {
	if s.isDone() {
		return 0, net.ErrClosed
	}
	budget := s.b.probeBudget()
	pctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()

	if !s.acquireWrite(pctx) {
		if s.isDone() {
			return 0, net.ErrClosed
		}
		if ctx.Err() != nil {
			return 0, ctx.Err()
		}
		if s.hasRecentBodyProgress(recentActivityWindow) {
			s.b.mu.Lock()
			cached := s.link
			s.b.mu.Unlock()
			return time.Duration(cached.latencyMs) * time.Millisecond, nil
		}
		s.closeWith("timeout")
		return 0, context.DeadlineExceeded
	}

	id := randHex(8)
	ch := make(chan struct{})
	s.probeMu.Lock()
	if s.probes == nil {
		s.probes = map[string]chan struct{}{}
	}
	s.probes[id] = ch
	s.probeMu.Unlock()
	defer func() {
		s.probeMu.Lock()
		delete(s.probes, id)
		s.probeMu.Unlock()
	}()

	start := time.Now()
	_ = s.conn.SetWriteDeadline(time.Now().Add(budget))
	err := s.w.WriteJSON(wire.Ping{T: "ping", ID: id})
	_ = s.conn.SetWriteDeadline(time.Time{})
	s.releaseWrite()
	if err != nil {
		if !s.isDone() {
			s.closeWith(reason(err))
		}
		return 0, err
	}
	s.markTx()

	select {
	case <-ch:
		return time.Since(start), nil
	case <-s.done:
		return 0, net.ErrClosed
	case <-ctx.Done():
		return 0, ctx.Err()
	case <-pctx.Done():
		// A large incoming body may delay the peer's pong frame; if inbound
		// traffic progressed during the wait, the session is demonstrably alive.
		if s.hasRxSince(start) || s.hasRecentBodyProgress(recentActivityWindow) {
			s.b.mu.Lock()
			cached := s.link
			s.b.mu.Unlock()
			return time.Duration(cached.latencyMs) * time.Millisecond, nil
		}
		s.closeWith("timeout")
		return 0, context.DeadlineExceeded
	}
}

func (b *Backend) classifyPath(ctx context.Context, s *session, active bool) string {
	if active {
		if prober, ok := b.cfg.Transport.(interface {
			ProbePath(context.Context, net.Conn, bool) string
		}); ok {
			dctx, cancel := context.WithTimeout(ctx, discoProbeTimeout)
			path := safeProbePath(prober, dctx, s.conn, s.outgoing)
			cancel()
			if rpc.ValidLinkPath(path) {
				return path
			}
			return rpc.LinkPathUnknown
		}
	}
	if tr, ok := b.cfg.Transport.(interface{ Path(net.Conn, bool) string }); ok {
		path := safePath(tr, s.conn, s.outgoing)
		if rpc.ValidLinkPath(path) {
			return path
		}
	}
	return rpc.LinkPathUnknown
}

func safeProbePath(tr interface {
	ProbePath(context.Context, net.Conn, bool) string
}, ctx context.Context, c net.Conn, out bool) (path string) {
	path = rpc.LinkPathUnknown
	defer func() { _ = recover() }()
	return tr.ProbePath(ctx, c, out)
}

// probeAndHeal warms up a live session after sleep/wake or network change and
// tears it down quickly if the underlying tunnel/socket is half-open.
func (b *Backend) probeAndHeal(s *session) {
	if s == nil || s.isDone() {
		return
	}
	ctx := s.r.ctx
	rtt, err := s.ping(ctx)
	if err != nil {
		return
	}
	path := b.classifyPath(ctx, s, false)
	nowMs := time.Now().UnixMilli()
	rttMs := max(rtt.Milliseconds(), int64(0))
	if rtt > 0 && rttMs == 0 {
		rttMs = 1
	}
	b.mu.Lock()
	if b.sessions[s.peerID] == s && !s.isDone() {
		if path != "" && path != rpc.LinkPathUnknown {
			s.link.path = path
		}
		s.link.latencyMs = rttMs
		s.link.lastCheckedMs = nowMs
	}
	b.mu.Unlock()
	b.notifyLinkChanged(s.peerID)
}

// ProbePeers explicitly checks the requested peers (or all configured peers
// when peerIDs is empty). Live sessions get an application-layer ping/pong plus
// active transport path classification (including outgoing DiscoPing for tailcat).
// Disconnected peers immediately reset dial backoff and start reconnecting.
func (b *Backend) ProbePeers(ctx context.Context, peerIDs []string) ([]rpc.PeerLinkStatus, error) {
	b.mu.Lock()
	r := b.run
	peers := append([]rpc.StatePeer(nil), b.st.Peers...)
	b.mu.Unlock()
	if r == nil {
		return nil, rpc.NewError(rpc.CodeUnavailable)
	}

	want := make(map[string]bool, len(peerIDs))
	for _, id := range peerIDs {
		if id != "" {
			want[id] = true
		}
	}
	targets := make([]string, 0, len(peers))
	for _, p := range peers {
		if len(want) == 0 || want[p.ID] {
			targets = append(targets, p.ID)
		}
	}
	if len(targets) == 0 {
		return nil, nil
	}

	sem := make(chan struct{}, maxProbeConcurrency)
	results := make([]rpc.PeerLinkStatus, len(targets))
	var wg sync.WaitGroup
	for i, peerID := range targets {
		i, peerID := i, peerID
		wg.Add(1)
		go func() {
			defer wg.Done()
			select {
			case sem <- struct{}{}:
				defer func() { <-sem }()
			case <-ctx.Done():
				results[i] = b.peerStatus(peerID)
				return
			}
			results[i] = b.probeOnePeer(ctx, peerID)
		}()
	}
	wg.Wait()
	return results, nil
}

func (b *Backend) probeOnePeer(ctx context.Context, peerID string) rpc.PeerLinkStatus {
	b.mu.Lock()
	s := b.sessions[peerID]
	b.mu.Unlock()

	nowMs := time.Now().UnixMilli()
	if s != nil && !s.isDone() {
		rtt, err := s.ping(ctx)
		if err == nil {
			path := b.classifyPath(ctx, s, true)
			rttMs := max(rtt.Milliseconds(), int64(0))
			if rtt > 0 && rttMs == 0 {
				rttMs = 1
			}
			b.mu.Lock()
			if b.sessions[peerID] == s && !s.isDone() {
				s.link = peerLinkCache{
					path:          path,
					latencyMs:     rttMs,
					lastCheckedMs: nowMs,
				}
			}
			b.mu.Unlock()
			b.notifyLinkChanged(peerID)
			return rpc.PeerLinkStatus{
				PeerID:        peerID,
				State:         rpc.LinkStateConnected,
				Path:          path,
				LatencyMs:     rttMs,
				LastCheckedMs: nowMs,
			}
		}
		if errors.Is(err, context.Canceled) && ctx.Err() != nil {
			return b.peerStatus(peerID)
		}
	}

	// Peer is disconnected or its stale session just closed: reset backoff and dial now.
	b.resetDialAndKick(peerID, nowMs)
	st := b.peerStatus(peerID)
	st.LastCheckedMs = nowMs
	return st
}

func (b *Backend) resetDialAndKick(peerID string, checkedMs int64) {
	b.mu.Lock()
	if b.run != nil {
		ds := b.dialing[peerID]
		ds.owner = nil
		ds.inFlight = false
		ds.attempts = 0
		ds.activeUntil = time.Now().Add(connectingGraceWindow)
		ds.visibleConnecting = true
		b.dialing[peerID] = ds
		if checkedMs > 0 {
			b.lastChecked[peerID] = checkedMs
		}
		if d := b.dialers[peerID]; d != nil {
			// Resetting the dialer goroutine clears any long backoff timer immediately.
			p := rpc.StatePeer{}
			for _, sp := range b.st.Peers {
				if sp.ID == peerID {
					p = sp
					break
				}
			}
			r := b.run
			if p.Address != "" {
				d.cancel()
				delete(b.dialers, peerID)
				b.mu.Unlock()
				b.keepDialing(r, p)
				b.notifyLinkChanged(peerID)
				return
			}
		}
	}
	b.mu.Unlock()
	b.notifyLinkChanged(peerID)
}

func (b *Backend) peerStatus(peerID string) rpc.PeerLinkStatus {
	snaps := b.LinkSnapshots()
	if st, ok := snaps[peerID]; ok {
		return st
	}
	return rpc.PeerLinkStatus{PeerID: peerID, State: rpc.LinkStateDisconnected}
}

// LinkSnapshots returns a non-blocking snapshot of every configured peer's
// connection state, cached/passive path classification, RTT, and last check time.
func (b *Backend) LinkSnapshots() map[string]rpc.PeerLinkStatus {
	now := time.Now()
	b.mu.Lock()
	r := b.run
	peers := append([]rpc.StatePeer(nil), b.st.Peers...)
	live := make(map[string]*session, len(b.sessions))
	for id, s := range b.sessions {
		if s != nil && !s.isDone() {
			live[id] = s
		}
	}
	b.mu.Unlock()

	var passiveSnapshot func(net.Conn, bool) string
	for _, s := range live {
		if !s.outgoing {
			if tr, ok := b.cfg.Transport.(interface {
				PathSnapshot() func(net.Conn, bool) string
			}); ok {
				passiveSnapshot = safePathSnapshot(tr)
			}
			break
		}
	}

	out := make(map[string]rpc.PeerLinkStatus, len(peers))
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, p := range peers {
		s := live[p.ID]
		if s != nil && !s.isDone() {
			path := s.link.path
			if passiveSnapshot != nil && !s.outgoing {
				if ppath := safePath(pathFunction(passiveSnapshot), s.conn, false); ppath != "" && ppath != rpc.LinkPathUnknown {
					path = ppath
				}
			}
			out[p.ID] = rpc.PeerLinkStatus{
				PeerID:        p.ID,
				State:         rpc.LinkStateConnected,
				Path:          path,
				LatencyMs:     s.link.latencyMs,
				LastCheckedMs: s.link.lastCheckedMs,
			}
			continue
		}
		state := rpc.LinkStateDisconnected
		if r != nil && b.dialConnectingLocked(p.ID, now) {
			state = rpc.LinkStateConnecting
		}
		out[p.ID] = rpc.PeerLinkStatus{
			PeerID:        p.ID,
			State:         state,
			LastCheckedMs: b.lastChecked[p.ID],
		}
	}
	return out
}
