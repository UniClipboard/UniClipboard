package engine

import (
	"context"
	"crypto/sha256"
	"errors"
	"strings"
	"time"
	"unicode"

	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/internal/tcnet"
)

// pairTimeout bounds one pairing attempt (dial plus handshake).
const pairTimeout = 45 * time.Second

// codeTimer tracks the pairing code this device shows, only to announce its
// expiry; the code itself lives in the session layer.
type codeTimer struct {
	gen     uint64
	active  bool
	expires time.Time
	timer   *time.Timer
}

func (c *codeTimer) stop() {
	if c.timer != nil {
		c.timer.Stop()
		c.timer = nil
	}
}

func (c *codeTimer) clear() { c.stop(); c.active = false; c.gen++ }

func (c *codeTimer) state(now time.Time) PairingCodeState {
	if c.active && now.Before(c.expires) {
		return PairingCodeState{Active: true, ExpiresAtMs: c.expires.UnixMilli()}
	}
	return PairingCodeState{}
}

// callCtx ends when either ctx or the session layer's run ends, so that a
// suspend or stop does not leave a call waiting on the network.
func (e *Engine) callCtx(ctx context.Context) (context.Context, context.CancelFunc) {
	e.mu.Lock()
	rc := e.runCtx
	e.mu.Unlock()
	c, cancel := context.WithCancel(ctx)
	if rc == nil {
		return c, cancel
	}
	stop := context.AfterFunc(rc, cancel)
	return c, func() { stop(); cancel() }
}

// mapNetErr turns a failure of the session layer into a redacted engine error.
// The text of err is never forwarded: it may hold addresses or tickets.
func mapNetErr(err error) *Error {
	var ee *Error
	switch {
	case errors.As(err, &ee):
		return ee
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return ctxErr(err)
	case errors.Is(err, session.ErrSelf):
		return newErr(CodeInvalidArgument, "that is this device itself")
	case errors.Is(err, tcnet.ErrBadAddress):
		return newErr(CodeInvalidArgument, "that is not a valid device address")
	case errors.Is(err, session.ErrProtocol):
		return newErr(CodeProtocolMismatch, "the other device uses an incompatible protocol version")
	case errors.Is(err, session.ErrIdentity):
		return newErr(CodeIdentityMismatch, "the address belongs to a different device")
	case errors.Is(err, session.ErrNotRunning):
		return newErr(CodeInvalidState, "the engine is not running")
	}
	if ce, ok := codeFailure(err); ok {
		return ce
	}
	return retryable(newErr(CodeUnavailable, "could not reach the device"))
}

// ---- pairing codes ----

// CreatePairingCode shows a new code (replacing the previous one only if this
// succeeds). The code finds this device; it grants nobody any trust.
func (e *Engine) CreatePairingCode(ctx context.Context) (PairingCode, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return PairingCode{}, gerr
	}
	defer release()
	e.mu.Lock()
	nerr := e.needNetworkLocked()
	e.mu.Unlock()
	if nerr != nil {
		return PairingCode{}, nerr
	}
	cctx, cancel := e.callCtx(ctx)
	defer cancel()
	c, err := e.core.IssueCode(cctx)
	if err != nil {
		return PairingCode{}, mapNetErr(err)
	}
	e.mu.Lock()
	e.code.clear()
	e.code.active, e.code.expires = true, c.Expires
	gen := e.code.gen
	e.code.timer = time.AfterFunc(time.Until(c.Expires), func() {
		e.mu.Lock()
		defer e.mu.Unlock()
		if e.code.gen == gen && e.code.active {
			e.code.active = false
			e.commitLocked(EventPairingCode)
		}
	})
	e.commitLocked(EventPairingCode)
	e.mu.Unlock()
	return PairingCode{Code: c.Code, ExpiresAtMs: c.Expires.UnixMilli()}, nil
}

// GetPairingCode returns the code being shown, if it is still valid.
func (e *Engine) GetPairingCode(ctx context.Context) (PairingCode, bool, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return PairingCode{}, false, gerr
	}
	defer release()
	c, ok := e.core.CurrentCode()
	if !ok {
		return PairingCode{}, false, nil
	}
	return PairingCode{Code: c.Code, ExpiresAtMs: c.Expires.UnixMilli()}, true, nil
}

// CancelPairingCode withdraws the code. It is invalid locally when this
// returns; telling the code service is best effort.
func (e *Engine) CancelPairingCode(ctx context.Context) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.core.CancelCode()
	e.mu.Lock()
	if e.code.active {
		e.code.clear()
		e.commitLocked(EventPairingCode)
	}
	e.mu.Unlock()
	return nil
}

// ExportPairingAddress returns this device's address for pairing by hand. It
// is a secret; show it only when the user asks.
func (e *Engine) ExportPairingAddress(ctx context.Context) (SensitiveAddress, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return SensitiveAddress{}, gerr
	}
	defer release()
	e.mu.Lock()
	life := e.life
	e.mu.Unlock()
	if life != LifecycleRunning {
		return SensitiveAddress{}, newErr(CodeInvalidState, "the engine is not running")
	}
	_, _, addr := e.core.Self()
	if addr == "" {
		return SensitiveAddress{}, retryable(newErr(CodeUnavailable, "the address is not ready yet"))
	}
	return SensitiveAddress{Address: addr}, nil
}

// ---- pairing operations ----

// JoinByCode pairs with the device showing a code. The result arrives through
// the returned operation. The other device must still approve this one.
func (e *Engine) JoinByCode(ctx context.Context, req JoinRequest) (OperationID, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	if req.RequestID == "" {
		return "", newErr(CodeInvalidArgument, "RequestID is required")
	}
	code, ok := rendezvous.NormalizeCode(req.Code)
	if !ok {
		return "", newErr(CodeIncomplete, "enter all 6 digits of the code")
	}
	return e.startPairing(req.RequestID, "join\x00"+code, PhaseResolving, func(ctx context.Context, op OperationID) (peer DeviceID, awaiting bool, err error) {
		p, aw, err := e.core.JoinWith(ctx, code, func() {
			e.updateOp(op, func(o *Operation) { o.Phase = PhaseConnecting })
		})
		return DeviceID(p.ID), aw, err
	})
}

// PairByAddress pairs with the device at a full address (the way that needs no
// code service). The other device must still approve this one.
func (e *Engine) PairByAddress(ctx context.Context, req PairRequest) (OperationID, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	if req.RequestID == "" {
		return "", newErr(CodeInvalidArgument, "RequestID is required")
	}
	addr := strings.TrimSpace(req.Address)
	if addr == "" || len(addr) > 2048 || strings.IndexFunc(addr, unicode.IsSpace) >= 0 || strings.IndexFunc(addr, unicode.IsControl) >= 0 {
		return "", newErr(CodeInvalidArgument, "that is not a valid device address")
	}
	return e.startPairing(req.RequestID, "pair\x00"+addr, PhaseConnecting, func(ctx context.Context, _ OperationID) (DeviceID, bool, error) {
		p, aw, err := e.core.AddPeer(ctx, addr)
		return DeviceID(p.ID), aw, err
	})
}

// startPairing registers the operation (or finds the one this RequestID made)
// and runs work in the background. The sensitive parameter is only hashed.
func (e *Engine) startPairing(req RequestID, param string, phase Phase,
	work func(ctx context.Context, op OperationID) (DeviceID, bool, error)) (OperationID, error) {

	hash := sha256.Sum256([]byte(param))
	e.mu.Lock()
	if nerr := e.needNetworkLocked(); nerr != nil {
		// A repeated request still gets its original answer.
		if old := e.ops.reqs[req]; old != nil && old.hash == hash {
			id := old.op.ID
			e.mu.Unlock()
			return id, nil
		}
		e.mu.Unlock()
		return "", nerr
	}
	o, existing, oerr := e.newOpLocked(OperationPairing, req, hash, phase)
	if oerr != nil {
		e.mu.Unlock()
		return "", oerr
	}
	if existing {
		id := o.op.ID
		e.mu.Unlock()
		return id, nil
	}
	id, rctx := o.op.ID, e.runCtx
	e.opWG.Add(1)
	e.mu.Unlock()

	go func() {
		defer e.opWG.Done()
		ctx, cancel := context.WithTimeout(rctx, pairTimeout)
		defer cancel()
		peer, awaiting, err := work(ctx, id)
		if err != nil {
			ee := mapNetErr(err)
			e.updateOp(id, func(o *Operation) { o.Phase, o.Error = PhaseFailed, ee })
			return
		}
		fp := session.Fingerprint(string(e.self), string(peer))
		e.updateOp(id, func(o *Operation) {
			o.Phase = PhaseSucceeded
			o.Pairing = &PairingResult{PeerID: peer, Fingerprint: fp, AwaitingRemoteApproval: awaiting}
		})
	}()
	return id, nil
}

// GetOperation returns an operation while it is retained.
func (e *Engine) GetOperation(ctx context.Context, id OperationID) (Operation, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return Operation{}, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	e.ops.prune(time.Now(), e.res.OperationTTL)
	o := e.ops.byID[id]
	if o == nil {
		if e.foreign(string(id)) {
			return Operation{}, newErr(CodeResourceExpired, "the operation belongs to an earlier instance")
		}
		return Operation{}, newErr(CodeNotFound, "no such operation (it may have expired)")
	}
	return o.op.clone(), nil
}

// ---- devices ----

func (e *Engine) peers() ([]Peer, *Error) {
	ps, err := e.core.Peers()
	if err != nil {
		return nil, newErr(CodeUnavailable, "cannot read the paired devices")
	}
	out := make([]Peer, 0, len(ps))
	for _, p := range ps {
		c := Disconnected
		if p.Connected {
			c = Connected
		}
		out = append(out, Peer{ID: DeviceID(p.ID), Name: p.Name, TrustRevision: uint64(p.AddedAt), Connection: c, LastSeenMs: p.LastSeen * 1000})
	}
	return out, nil
}

// pending lists the requests waiting for the user. A request's revision
// changes whenever the device asks again, so an approval of a stale view fails.
func (e *Engine) pending() ([]PendingPeer, *Error) {
	ps, err := e.core.Pending()
	if err != nil {
		return nil, newErr(CodeUnavailable, "cannot read the pending devices")
	}
	e.pmu.Lock()
	defer e.pmu.Unlock()
	out := make([]PendingPeer, 0, len(ps))
	live := map[DeviceID]bool{}
	for _, p := range ps {
		id := DeviceID(p.ID)
		live[id] = true
		r := e.pendRevs[id]
		if r.rev == 0 || r.seen != p.Time {
			e.pendCount++
			r = pendRev{seen: p.Time, rev: e.pendCount}
			e.pendRevs[id] = r
		}
		src := SourceDirect
		switch {
		case p.ByCode:
			src = SourceCode
		case p.Via != "":
			src = SourceIntroduction
		}
		out = append(out, PendingPeer{ID: id, Name: p.Name, RequestRevision: r.rev, Source: src,
			IntroducedByName: p.Via, Fingerprint: p.Fingerprint, SeenAtMs: p.Time * 1000})
	}
	for id := range e.pendRevs {
		if !live[id] {
			delete(e.pendRevs, id)
		}
	}
	return out, nil
}

// ListPeers returns the paired devices.
func (e *Engine) ListPeers(ctx context.Context) ([]Peer, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	ps, err := e.peers()
	if err != nil {
		return nil, err
	}
	return ps, nil
}

// ListPending returns the devices waiting for approval.
func (e *Engine) ListPending(ctx context.Context) ([]PendingPeer, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	ps, err := e.pending()
	if err != nil {
		return nil, err
	}
	return ps, nil
}

// findPending returns the pending request that req names, or the error to give.
func (e *Engine) findPending(id DeviceID, rev uint64) (PendingPeer, *Error) {
	ps, err := e.pending()
	if err != nil {
		return PendingPeer{}, err
	}
	for _, p := range ps {
		if p.ID == id {
			if p.RequestRevision != rev {
				return p, newErr(CodeConflict, "the request changed; look at it again")
			}
			return p, nil
		}
	}
	return PendingPeer{}, newErr(CodeNotFound, "no such pending device")
}

// Approve trusts a pending device. It checks that the user approved the
// request they saw: the revision and the fingerprint must still match. The
// decision is stored before Approve returns; connecting happens afterwards.
func (e *Engine) Approve(ctx context.Context, req ApprovalRequest) (Peer, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return Peer{}, gerr
	}
	defer release()
	p, perr := e.findPending(req.DeviceID, req.RequestRevision)
	if perr != nil {
		return Peer{}, perr
	}
	if req.ExpectedFingerprint == "" || req.ExpectedFingerprint != p.Fingerprint {
		return Peer{}, newErr(CodeConflict, "the fingerprint does not match; compare it again")
	}
	if _, err := e.core.Approve(string(req.DeviceID)); err != nil {
		if errors.Is(err, session.ErrNoPending) {
			return Peer{}, newErr(CodeNotFound, "no such pending device")
		}
		return Peer{}, newErr(CodeUnavailable, "cannot store the decision")
	}
	ps, err := e.peers()
	if err != nil {
		return Peer{}, err
	}
	for _, q := range ps {
		if q.ID == req.DeviceID {
			return q, nil
		}
	}
	return Peer{}, newErr(CodeInternal, "the approved device vanished")
}

// Reject dismisses a pending device. Introductions do not bring it back; a
// connection attempt from the device itself does, as a new request.
func (e *Engine) Reject(ctx context.Context, req PendingRequest) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	if _, perr := e.findPending(req.DeviceID, req.RequestRevision); perr != nil {
		return perr
	}
	if err := e.core.Reject(string(req.DeviceID)); err != nil {
		if errors.Is(err, session.ErrNoPending) {
			return newErr(CodeNotFound, "no such pending device")
		}
		return newErr(CodeUnavailable, "cannot store the decision")
	}
	e.commit(EventPending)
	return nil
}

// Unpair removes a paired device and drops its connection. Unpairing a device
// that is not paired succeeds. Pairing again needs a new approval.
func (e *Engine) Unpair(ctx context.Context, id DeviceID) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	if _, err := e.core.RemovePeer(string(id)); err != nil {
		return newErr(CodeUnavailable, "cannot store the decision")
	}
	return nil
}
