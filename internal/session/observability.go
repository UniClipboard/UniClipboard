package session

import (
	"context"
	"errors"
	"io"
	"net"
	"time"

	"github.com/uniclipboard/unic/internal/syncdiag"
	"github.com/uniclipboard/unic/plugin/rpc"
)

type sessionObservation struct {
	epoch       uint64
	started     time.Time
	observed    time.Time
	sampled     time.Time
	path        string
	derpSince   time.Time
	firstKnown  bool
	upgradeDone bool
}
type recoveryObservation struct {
	epoch    uint64
	started  time.Time
	trustRev uint64
}

func (b *Backend) epoch() (v uint64) {
	if b.cfg.Observer == nil {
		return 0
	}
	defer func() { _ = recover() }()
	return b.cfg.Observer.SyncEpoch()
}
func (b *Backend) emit(r syncdiag.Record) {
	if r.Epoch == 0 || b.cfg.Observer == nil {
		return
	}
	defer func() { _ = recover() }()
	b.cfg.Observer.RecordSync(r)
}
func (b *Backend) wakeObservation() {
	if b.cfg.Observer == nil {
		return
	}
	defer func() { _ = recover() }()
	b.cfg.Observer.WakeSync()
}
func reason(err error) string {
	if err == nil {
		return "unknown"
	}
	var ne net.Error
	var re *rpc.Error
	switch {
	case errors.Is(err, context.Canceled):
		return "cancellation"
	case errors.Is(err, context.DeadlineExceeded):
		return "timeout"
	case errors.As(err, &ne) && ne.Timeout():
		return "timeout"
	case errors.Is(err, ErrBadAddress):
		return "invalid_address"
	case errors.Is(err, ErrProtocol):
		return "protocol_mismatch"
	case errors.Is(err, io.EOF):
		return "remote_close"
	case errors.As(err, &re):
		switch re.Code {
		case "identity_mismatch", "bad_proof", "bad_pair_proof", "binding_mismatch", "replay":
			return "identity_mismatch"
		case "protocol_mismatch":
			return "protocol_mismatch"
		case "permission_denied":
			return "permission_rejection"
		case "cancelled":
			return "cancellation"
		case "timeout":
			return "timeout"
		case "unavailable":
			return "unavailable"
		case "bad_credentials":
			return "unusable_credentials"
		}
	case errors.As(err, &ne), errors.Is(err, io.ErrUnexpectedEOF), errors.Is(err, io.ErrClosedPipe):
		return "io_failure"
	}
	return "unknown"
}
func (b *Backend) attempt(epoch uint64, phase, dir string, start time.Time, err error, result string) {
	if result == "" {
		result = "succeeded"
		if err != nil {
			result = "failed"
			if reason(err) == "cancellation" {
				result = "cancelled"
			}
		}
	}
	b.emit(syncdiag.Record{Epoch: epoch, Kind: "connection", Phase: phase, Direction: dir, Result: result, Reason: reason(err), Duration: time.Since(start)})
}
func (b *Backend) dial(ctx context.Context, addr string) (net.Conn, error) {
	epoch, start := b.epoch(), time.Now()
	c, err := b.cfg.Transport.Dial(ctx, addr)
	b.attempt(epoch, "dial", "outgoing", start, err, "")
	return c, err
}

// Collect is called only by the service metrics worker. Snapshot reduction takes
// place outside the session state lock. A stale/missing sample never closes a session.
func (b *Backend) Collect(epoch uint64) (records []syncdiag.Record, more bool) {
	now := time.Now()
	if epoch == 0 {
		b.mu.Lock()
		for _, s := range b.sessions {
			s.observation = sessionObservation{}
		}
		b.recoveries = map[string]recoveryObservation{}
		b.mu.Unlock()
		return nil, false
	}
	b.mu.Lock()
	ss := make([]*session, 0, len(b.sessions))
	for _, s := range b.sessions {
		if !s.isDone() {
			ss = append(ss, s)
		}
	}
	b.mu.Unlock()
	var snapshot func(net.Conn, bool) string
	for _, s := range ss {
		if !s.outgoing {
			if tr, ok := b.cfg.Transport.(interface {
				PathSnapshot() func(net.Conn, bool) string
			}); ok {
				snapshot = safePathSnapshot(tr)
			}
			break
		}
	}
	for _, s := range ss {
		path := "unknown"
		if snapshot != nil {
			path = safePath(pathFunction(snapshot), s.conn, s.outgoing)
		} else if tr, ok := b.cfg.Transport.(interface{ Path(net.Conn, bool) string }); ok {
			path = safePath(tr, s.conn, s.outgoing)
		}
		if !syncdiag.Path(path) {
			path = "unknown"
		}
		b.mu.Lock()
		if b.sessions[s.peerID] != s || s.isDone() {
			b.mu.Unlock()
			continue
		}
		o := &s.observation
		if o.epoch != epoch {
			*o = sessionObservation{epoch: epoch, started: now, observed: now}
		}
		records = append(records, syncdiag.Record{Epoch: epoch, Kind: "exposure", Duration: now.Sub(o.observed)})
		o.observed = now
		dir := "incoming"
		if s.outgoing {
			dir = "outgoing"
		}
		records = append(records, syncdiag.Record{Epoch: epoch, Kind: "path", Direction: dir, Path: path})

		if syncdiag.KnownPath(path) {
			if syncdiag.KnownPath(o.path) && o.path != path {
				records = append(records, syncdiag.Record{Epoch: epoch, Kind: "transition", From: o.path, To: path})
			}
			if !o.firstKnown {
				o.firstKnown = true
				o.upgradeDone = path == "direct"
			}
			if path == "derp" && !o.upgradeDone && o.derpSince.IsZero() {
				o.derpSince = now
			}
			if path == "direct" && !o.derpSince.IsZero() {
				records = append(records, syncdiag.Record{Epoch: epoch, Kind: "upgrade", Duration: now.Sub(o.derpSince)})
				o.derpSince = time.Time{}
				o.upgradeDone = true
			}
		} else {
			o.derpSince = time.Time{}
		}

		o.path, o.sampled = path, now
		b.mu.Unlock()
	}
	b.mu.Lock()
	for id, r := range b.recoveries {
		if r.epoch != epoch {
			delete(b.recoveries, id)
			records = append(records, syncdiag.Record{Epoch: epoch, Kind: "recovery", Result: "interrupted"})
			continue
		}
		records = append(records, syncdiag.Record{Epoch: epoch, Kind: "pending", Duration: now.Sub(r.started)})
	}
	more = len(b.sessions) > 0 || len(b.recoveries) > 0
	b.mu.Unlock()
	return records, more
}

type pathFunction func(net.Conn, bool) string

func (f pathFunction) Path(c net.Conn, out bool) string { return f(c, out) }
func safePathSnapshot(tr interface {
	PathSnapshot() func(net.Conn, bool) string
}) (snapshot func(net.Conn, bool) string) {
	defer func() { _ = recover() }()
	return tr.PathSnapshot()
}

func safePath(tr interface{ Path(net.Conn, bool) string }, c net.Conn, out bool) (path string) {
	path = "unknown"
	defer func() { _ = recover() }()
	return tr.Path(c, out)
}
func (s *session) startPath(epoch uint64) string {
	s.b.mu.Lock()
	defer s.b.mu.Unlock()
	o := s.observation
	if epoch == 0 || o.epoch != epoch || o.sampled.IsZero() || time.Since(o.sampled) > 10*time.Second {
		return "unknown"
	}
	return o.path
}

// endObservation is called once by serve after the connection's close claim.
func (b *Backend) endObservation(s *session) {
	currentEpoch := b.epoch()
	s.endMu.Lock()
	epoch := s.endEpoch
	why := s.endReason
	now := s.ended
	s.endMu.Unlock()
	expected := why == "local_stop" || why == "suspension" || why == "revocation" || why == "state_replacement"
	b.mu.Lock()
	o := s.observation
	if b.sessions[s.peerID] == s {
		delete(b.sessions, s.peerID)
	}
	var recovered *syncdiag.Record
	var exposure time.Duration
	if o.epoch == epoch && !o.observed.IsZero() {
		exposure = now.Sub(o.observed)
	}
	eligible := false
	for _, p := range b.st.Peers {
		if p.ID == s.peerID && uint64(p.TrustRevision) == s.trustRev {
			eligible = true
			break
		}
	}
	if epoch != 0 && epoch == currentEpoch && !expected && b.run == s.r && eligible {
		if successor := b.sessions[s.peerID]; successor != nil {
			duration := successor.observation.started.Sub(now)
			if duration < 0 {
				duration = 0
			}
			recovered = &syncdiag.Record{Epoch: epoch, Kind: "recovery", Result: "recovered", Duration: duration}
		} else if _, ok := b.recoveries[s.peerID]; !ok {
			b.recoveries[s.peerID] = recoveryObservation{epoch: epoch, started: now, trustRev: s.trustRev}
		}
	}
	b.mu.Unlock()
	if epoch == 0 || epoch != currentEpoch {
		return
	}
	if recovered != nil {
		b.emit(*recovered)
	}
	// A session that spans a disabled window reports only this epoch's duration.
	duration := time.Duration(0)
	if o.epoch == epoch && !o.started.IsZero() {
		duration = now.Sub(o.started)
	}
	b.emit(syncdiag.Record{Epoch: epoch, Kind: "session", Reason: why, Expected: expected, Duration: duration})
	if exposure > 0 {
		b.emit(syncdiag.Record{Epoch: epoch, Kind: "exposure", Duration: exposure})
	}
	b.wakeObservation()
}
func (s *session) closeWith(why string) {
	epoch := s.b.epoch()
	s.once.Do(func() {
		s.endMu.Lock()
		s.endReason = why
		s.endEpoch = epoch
		s.ended = time.Now()
		s.endMu.Unlock()
		close(s.done)
		_ = s.conn.Close()
	})
}

// Body observation is local to one serial body stream.
type bodyObservation struct {
	first, last time.Time
	bytes       int64
}

func (o *bodyObservation) add(start time.Time, n int) {
	if o.first.IsZero() {
		o.first = start
	}
	o.last = time.Now()
	o.bytes += int64(n)
}
func (o *bodyObservation) duration() time.Duration {
	if o.first.IsZero() {
		return 0
	}
	return o.last.Sub(o.first)
}

type measuredReader struct {
	r io.Reader
	o *bodyObservation
}

func (r measuredReader) Read(p []byte) (int, error) {
	start := time.Now()
	n, err := r.r.Read(p)
	r.o.add(start, n)
	return n, err
}

func clipMeasurement(parts []rpc.ClipPart) (int64, string) {
	var size int64
	kind := ""
	for _, p := range parts {
		size += int64(p.Size)
		k := "text"
		if p.Kind == "file" {
			k = "files"
		} else if len(p.MIME) >= 6 && p.MIME[:6] == "image/" {
			k = "image"
		}
		if kind == "" {
			kind = k
		} else if kind != k {
			kind = "mixed"
		}
	}
	if kind == "" {
		kind = "unknown"
	}
	return size, kind
}
