package engine

import (
	"context"
	"crypto/subtle"
	"regexp"
	"sort"
	"time"

	"github.com/uniclipboard/unic/internal/peerbackend"
	"github.com/uniclipboard/unic/internal/store"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Candidates (decision 0012): devices an approved peer vouched for. They are
// suggestions. Nothing here grants trust; pairing with one is an explicit
// attempt like any other and the other device must approve it too.

const (
	candTTL        = 7 * 24 * time.Hour // how long a vouch counts after it was last repeated
	candRetryFor   = 24 * time.Hour     // how long a requested pairing keeps being retried
	candRetryFirst = time.Minute
	candRetryMax   = 15 * time.Minute
)

func tokenEqual(a, b string) bool { return subtle.ConstantTimeCompare([]byte(a), []byte(b)) == 1 }

var deviceIDRe = regexp.MustCompile(`^[a-z2-7]{26}$`)

// Candidate is a device that could be paired. Name is what the introducers
// call it and is not verified; the fingerprint is only known after contact.
type Candidate struct {
	DeviceID    string       `json:"device_id"`
	BackendID   PluginID     `json:"backend_id"`
	Name        string       `json:"name"`
	Introducers []Introducer `json:"introducers"`
	LastSeenMs  int64        `json:"last_seen_ms"` // when it was last vouched for
	Ignored     bool         `json:"ignored"`
	// Requested: the user asked to pair and the Engine keeps trying until
	// RequestedUntilMs.
	Requested        bool  `json:"requested"`
	RequestedUntilMs int64 `json:"requested_until_ms,omitempty"`
}

// Introducer is a trusted device that vouched for a candidate.
type Introducer struct {
	DeviceID string `json:"device_id"`
	Name     string `json:"name"`
}

type PairCandidateRequest struct {
	RequestID RequestID
	DeviceID  string
	BackendID PluginID
}

func (e *Engine) candidatesChanged(id string) {
	e.mu.Lock()
	e.commitObjsLocked([]ObjectRef{{Type: "candidate", ID: id}}, EventDevices)
	e.mu.Unlock()
}

// Introduce records the devices a connected, still approved peer vouches for.
// Names are never trusted: an invalid one is dropped, the rest kept.
func (b *peerProcBackend) Introduce(_ context.Context, r rpc.Introduce) error {
	b.am.Lock()
	b.sweepLocked(time.Now())
	s := b.sess[r.ConnectionID]
	if s == nil {
		b.am.Unlock()
		return rpcErr(rpc.CodeNotFound)
	}
	if !tokenEqual(s.token, r.SessionToken) {
		b.am.Unlock()
		return rpcErr(rpc.CodePermission)
	}
	if !s.connected || !s.ended.IsZero() {
		b.am.Unlock()
		return rpcErr(rpc.CodeInvalidState)
	}
	peer, want := s.peerID, s.trustRev
	b.am.Unlock()
	if rev, ok := b.trustRevision(peer); !ok || rev != want {
		return rpcErr(rpc.CodePermission)
	}
	e := b.e
	e.mu.Lock()
	on := e.settings.Value.Sync.Introductions
	e.mu.Unlock()
	if !on {
		return nil
	}
	self := e.ident.DeviceID()
	var cs []store.Candidate
	for _, c := range r.Candidates {
		if !deviceIDRe.MatchString(c.ID) || c.ID == self || c.ID == peer || c.Address == "" {
			continue
		}
		name := c.Name
		if !validDeviceName(name) {
			name = ""
		}
		cs = append(cs, store.Candidate{DeviceID: c.ID, Name: name, Address: []byte(c.Address)})
	}
	if err := e.st.ReplaceVouches(peer, string(b.id), cs, time.Now().UnixMilli(), candTTL.Milliseconds()); err != nil {
		return rpcErr(rpc.CodeInternal)
	}
	e.candidatesChanged(peer)
	return nil
}

// ListCandidates lists the devices vouched for, ignored ones included (marked).
func (e *Engine) ListCandidates(ctx context.Context) ([]Candidate, error) {
	release, gerr := e.historyGate(ctx, false, false)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	out, err := e.candidateList()
	if err != nil {
		return nil, newErr(CodeInternal, "could not read candidates")
	}
	return out, nil
}

func (e *Engine) candidateList() ([]Candidate, error) {
	now := time.Now()
	rows, err := e.st.Candidates(now.UnixMilli(), candTTL.Milliseconds())
	if err != nil {
		return nil, err
	}
	ign, err := e.st.IgnoredCandidates()
	if err != nil {
		return nil, err
	}
	reqs, err := e.st.CandidateRequests(now.UnixMilli(), candRetryFor.Milliseconds())
	if err != nil {
		return nil, err
	}
	asked := map[string]store.CandidateRequest{}
	for _, r := range reqs {
		asked[r.DeviceID] = r
	}
	byKey := map[string]*Candidate{}
	var order []string
	for _, r := range rows { // newest vouch first within a device and instance
		key := r.DeviceID + "\x00" + r.InstanceID
		c := byKey[key]
		if c == nil {
			c = &Candidate{DeviceID: r.DeviceID, BackendID: PluginID(r.InstanceID), Name: r.Name, LastSeenMs: r.SeenMs, Ignored: ign[r.DeviceID]}
			if q, ok := asked[r.DeviceID]; ok && q.InstanceID == r.InstanceID {
				c.Requested, c.RequestedUntilMs = true, q.RequestedMs+candRetryFor.Milliseconds()
			}
			byKey[key] = c
			order = append(order, key)
		}
		if r.SeenMs > c.LastSeenMs {
			c.LastSeenMs = r.SeenMs
		}
		name := ""
		if d, derr := e.st.Device(r.IntroducerID); derr == nil {
			name = Device{Name: d.Name, ReportedName: d.ReportedName}.DisplayName()
		}
		c.Introducers = append(c.Introducers, Introducer{DeviceID: r.IntroducerID, Name: name})
	}
	out := make([]Candidate, 0, len(order))
	for _, k := range order {
		out = append(out, *byKey[k])
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].LastSeenMs > out[j].LastSeenMs })
	return out, nil
}

// IgnoreCandidate hides a candidate for good (until restored). Restoring is
// ignored=false.
func (e *Engine) IgnoreCandidate(ctx context.Context, deviceID string, ignored bool) error {
	release, gerr := e.historyGate(ctx, true, false)
	if gerr != nil {
		return gerr
	}
	defer release()
	if !deviceIDRe.MatchString(deviceID) {
		return newErr(CodeInvalidArgument, "that is not a device ID")
	}
	if err := e.st.SetCandidateIgnored(deviceID, ignored, time.Now().UnixMilli()); err != nil {
		return newErr(CodeInternal, "could not save the choice")
	}
	if ignored {
		_ = e.st.ClearCandidateRequest(deviceID)
	}
	e.candidatesChanged(deviceID)
	return nil
}

// PairCandidate asks to pair with a candidate: one attempt now, and while the
// device is away more attempts for 24 hours. The device is not trusted by this;
// it shows up as any new device would and both sides still approve it.
func (e *Engine) PairCandidate(ctx context.Context, req PairCandidateRequest) (OperationID, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	if req.RequestID == "" || !deviceIDRe.MatchString(req.DeviceID) {
		return "", newErr(CodeInvalidArgument, "RequestID and a device ID are required")
	}
	rows, err := e.st.Candidates(time.Now().UnixMilli(), candTTL.Milliseconds())
	if err != nil {
		return "", newErr(CodeInternal, "could not read candidates")
	}
	var addr []byte
	for _, r := range rows {
		if r.DeviceID == req.DeviceID && r.InstanceID == string(req.BackendID) {
			addr = r.Address // the newest vouch comes first
			break
		}
	}
	if addr == nil {
		return "", newErr(CodeNotFound, "no such candidate (it may have expired)")
	}
	pp, pair := e.pairingBackend(req.BackendID)
	if pp == nil {
		return "", newErr(CodeUnsupported, "that sync backend cannot pair devices")
	}
	now := time.Now().UnixMilli()
	_ = e.st.SetCandidateIgnored(req.DeviceID, false, now)
	if err := e.st.RequestCandidate(req.DeviceID, string(req.BackendID), now); err != nil {
		return "", newErr(CodeInternal, "could not save the request")
	}
	e.candidatesChanged(req.DeviceID)
	return e.startPairing(req.RequestID, "candidate\x00"+req.DeviceID+"\x00"+string(req.BackendID), PhaseConnecting,
		func(ctx context.Context, _ OperationID) (st peerbackend.Standing, awaiting bool, err error) {
			st, awaiting, err = e.dialPeer(ctx, pp, pair, string(addr), req.DeviceID, nil)
			if err == nil {
				_ = e.st.ClearCandidateRequest(req.DeviceID)
				e.candidatesChanged(req.DeviceID)
			}
			return
		})
}

// candWait is the retry state of one requested candidate (memory only).
type candWait struct {
	next time.Time
	wait time.Duration
}

// retryCandidates makes another attempt for each requested candidate that is
// due. Called from the periodic sweep; every dial runs in the background.
func (e *Engine) retryCandidates() {
	now := time.Now()
	reqs, err := e.st.CandidateRequests(now.UnixMilli(), candRetryFor.Milliseconds())
	if err != nil || len(reqs) == 0 {
		return
	}
	rows, err := e.st.Candidates(now.UnixMilli(), candTTL.Milliseconds())
	if err != nil {
		return
	}
	for _, r := range reqs {
		if _, derr := e.st.Device(r.DeviceID); derr == nil {
			_ = e.st.ClearCandidateRequest(r.DeviceID) // it is a device now
			continue
		}
		var addr []byte
		for _, c := range rows {
			if c.DeviceID == r.DeviceID && c.InstanceID == r.InstanceID {
				addr = c.Address
				break
			}
		}
		pp, pair := e.pairingBackend(PluginID(r.InstanceID))
		if addr == nil || pp == nil {
			continue
		}
		e.mu.Lock()
		w := e.candRetry[r.DeviceID]
		due := e.life == LifecycleRunning && e.runtime == RuntimeActive && (w == nil || !now.Before(w.next))
		if due {
			if w == nil {
				w = &candWait{wait: candRetryFirst}
				if e.candRetry == nil {
					e.candRetry = map[string]*candWait{}
				}
				e.candRetry[r.DeviceID] = w
			}
			w.next = now.Add(w.wait)
			w.wait = min(w.wait*2, candRetryMax)
		}
		rctx := e.runCtx
		e.mu.Unlock()
		if !due || !e.startPeerWork() {
			continue
		}
		go func(id string, addr []byte) {
			defer e.opWG.Done()
			ctx, cancel := context.WithTimeout(rctx, pairTimeout)
			defer cancel()
			if _, _, err := e.dialPeer(ctx, pp, pair, string(addr), id, nil); err == nil {
				_ = e.st.ClearCandidateRequest(id)
				e.mu.Lock()
				delete(e.candRetry, id)
				e.mu.Unlock()
				e.reconcile()
				e.candidatesChanged(id)
			}
		}(r.DeviceID, addr)
	}
}
