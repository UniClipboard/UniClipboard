package engine

import (
	"context"
	"crypto/sha256"
	"errors"
	"strings"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/session"
)

// pairTimeout bounds one pairing attempt (dial plus handshake).
const pairTimeout = 45 * time.Second

// codeTimer tracks the pairing code this device shows, only to announce its
// expiry; the ticket it stands for is e.tix.
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
func mapNetErr(err error) *Error {
	var ee *Error
	switch {
	case errors.As(err, &ee):
		return ee
	case errors.Is(err, context.Canceled), errors.Is(err, context.DeadlineExceeded):
		return ctxErr(err)
	case errors.Is(err, session.ErrSelf):
		return newErr(CodeInvalidArgument, "that is this device itself")
	case errors.Is(err, session.ErrBadAddress):
		return newErr(CodeInvalidArgument, "that is not a valid pairing ticket")
	case errors.Is(err, session.ErrProtocol):
		return newErr(CodeProtocolMismatch, "the other device uses an incompatible protocol version")
	case errors.Is(err, session.ErrIdentity), errors.Is(err, session.ErrBadProof):
		return newErr(CodeIdentityMismatch, "the other device is not the one the ticket names")
	case errors.Is(err, session.ErrNotRunning):
		return newErr(CodeInvalidState, "the engine is not running")
	}
	if ce, ok := codeFailure(err); ok {
		return ce
	}
	return retryable(newErr(CodeUnavailable, "could not reach the device"))
}

// ---- tickets ----

// liveTicket is the one pairing ticket this device offers. It dies on first
// use, on expiry, or when replaced. code is the rendezvous code standing for
// it, "" if it was only exported as text. Guarded by Engine.mu.
type liveTicket struct {
	tok     []byte
	expires time.Time
	code    string
	text    string
}

// rv is the code service client, following rendezvous_url.
func (e *Engine) rv() *rendezvous.Client {
	e.mu.Lock()
	defer e.mu.Unlock()
	u := strings.TrimRight(e.settings.Value.RendezvousURL, "/")
	if u == "" {
		return e.rvDefault
	}
	if e.rvCache == nil || e.rvCache.URL != u {
		e.rvCache = rendezvous.NewWithURL(u)
	}
	return e.rvCache
}

// consumeCode tells the code service a code is used up. Best effort, in the
// background; Close waits for it with the other operations.
func (e *Engine) consumeCode(code string) {
	if code == "" {
		return
	}
	rv := e.rv()
	e.mu.Lock()
	if e.closing {
		e.mu.Unlock()
		return
	}
	parent := e.runCtx
	e.opWG.Add(1)
	e.mu.Unlock()
	go func() {
		defer e.opWG.Done()
		if parent == nil {
			parent = context.Background()
		}
		ctx, cancel := context.WithTimeout(parent, 10*time.Second)
		defer cancel()
		rv.Consume(ctx, code)
	}()
}

// newTicketLocked makes a fresh ticket for this device and its tailcat
// address. e.mu must be held.
func (e *Engine) newTicketLocked(expires time.Time) (rendezvous.Ticket, *Error) {
	addr := e.core.Addr()
	if addr == "" {
		return rendezvous.Ticket{}, retryable(newErr(CodeUnavailable, "the address is not ready yet"))
	}
	return rendezvous.Ticket{Transport: rendezvous.TransportPeerSessions, Device: e.ident.Public(),
		Access: []byte(addr), Token: rendezvous.NewToken()}, nil
}

// pairUsed is the session layer's question whether a peer's PairProof belongs
// to the live ticket. A match uses the ticket up: one ticket, one device.
func (e *Engine) pairUsed(t [32]byte, proof []byte) bool {
	e.mu.Lock()
	lt := e.tix
	if lt == nil || !time.Now().Before(lt.expires) || identity.VerifyPairProof(lt.tok, t, proof) != nil {
		e.mu.Unlock()
		return false
	}
	e.tix = nil
	code := lt.code
	if e.code.active {
		e.code.clear()
		e.commitLocked(EventPairingCode)
	}
	e.mu.Unlock()
	e.consumeCode(code)
	return true
}

// CreatePairingCode shows a new code (replacing the previous ticket only if
// this succeeds). The code finds this device; it grants nobody any trust.
func (e *Engine) CreatePairingCode(ctx context.Context) (PairingCode, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return PairingCode{}, gerr
	}
	defer release()
	e.mu.Lock()
	nerr := e.needNetworkLocked()
	var t rendezvous.Ticket
	if nerr == nil {
		t, nerr = e.newTicketLocked(time.Time{})
	}
	e.mu.Unlock()
	if nerr != nil {
		return PairingCode{}, nerr
	}
	rv := e.rv()
	cctx, cancel := e.callCtx(ctx)
	defer cancel()
	c, err := rv.Create(cctx, t)
	if err != nil {
		return PairingCode{}, mapNetErr(err)
	}
	e.mu.Lock()
	var oldCode string
	if e.tix != nil {
		oldCode = e.tix.code
	}
	e.tix = &liveTicket{tok: t.Token, expires: c.Expires, code: c.Code, text: t.Text()}
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
	if oldCode != "" {
		e.consumeCode(oldCode)
	}
	return PairingCode{Code: c.Code, ExpiresAtMs: c.Expires.UnixMilli()}, nil
}

// GetPairingCode returns the code being shown, if it is still valid.
func (e *Engine) GetPairingCode(ctx context.Context) (PairingCode, bool, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return PairingCode{}, false, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	t := e.tix
	if t == nil || t.code == "" || !time.Now().Before(t.expires) {
		return PairingCode{}, false, nil
	}
	return PairingCode{Code: t.code, ExpiresAtMs: t.expires.UnixMilli()}, true, nil
}

// CancelPairingCode withdraws the code and its ticket. They are invalid
// locally when this returns; telling the code service is best effort.
func (e *Engine) CancelPairingCode(ctx context.Context) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	e.mu.Lock()
	var code string
	if e.tix != nil {
		code = e.tix.code
		e.tix = nil
	}
	if e.code.active {
		e.code.clear()
		e.commitLocked(EventPairingCode)
	}
	e.mu.Unlock()
	e.consumeCode(code)
	return nil
}

// ExportPairingTicket returns the live ticket as text, for pairing without
// the code service (copy, command line, QR). It makes a ticket if none is live.
// The text is a secret equal to a password; show it only when the user asks.
func (e *Engine) ExportPairingTicket(ctx context.Context) (SensitiveTicket, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return SensitiveTicket{}, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	if nerr := e.needNetworkLocked(); nerr != nil {
		return SensitiveTicket{}, nerr
	}
	now := time.Now()
	if t := e.tix; t != nil && now.Before(t.expires) {
		return SensitiveTicket{Ticket: t.text, ExpiresAtMs: t.expires.UnixMilli()}, nil
	}
	t, nerr := e.newTicketLocked(now)
	if nerr != nil {
		return SensitiveTicket{}, nerr
	}
	exp := now.Add(rendezvous.MaxLifetime)
	e.tix = &liveTicket{tok: t.Token, expires: exp, text: t.Text()}
	return SensitiveTicket{Ticket: t.Text(), ExpiresAtMs: exp.UnixMilli()}, nil
}

// ---- joining ----

// JoinByCode pairs with the device showing a code. The result arrives through
// the returned operation. Both users must still approve each other.
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
	return e.startPairing(req.RequestID, "join\x00"+code, PhaseResolving, func(ctx context.Context, op OperationID) (session.Standing, bool, error) {
		t, err := e.rv().Resolve(ctx, code)
		if err != nil {
			return session.Standing{}, false, err
		}
		e.updateOp(op, func(o *Operation) { o.Phase = PhaseConnecting })
		return e.joinTicket(ctx, t)
	})
}

// JoinByTicket pairs with the device that issued a full ticket (the way that
// needs no code service). Both users must still approve each other.
func (e *Engine) JoinByTicket(ctx context.Context, req JoinTicketRequest) (OperationID, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return "", gerr
	}
	defer release()
	if req.RequestID == "" {
		return "", newErr(CodeInvalidArgument, "RequestID is required")
	}
	t, ok := rendezvous.ParseText(req.Ticket)
	if !ok {
		return "", newErr(CodeInvalidArgument, "that is not a valid pairing ticket")
	}
	return e.startPairing(req.RequestID, "ticket\x00"+t.Encode(), PhaseConnecting, func(ctx context.Context, _ OperationID) (session.Standing, bool, error) {
		return e.joinTicket(ctx, t)
	})
}

// joinTicket connects to the issuer of t, which must be the very device the
// ticket names. Only the tailcat backend handles peer sessions today; a ticket
// for another transport has no compatible backend and is refused, never
// handed to tailcat.
func (e *Engine) joinTicket(ctx context.Context, t rendezvous.Ticket) (session.Standing, bool, error) {
	if t.Transport != rendezvous.TransportPeerSessions {
		return session.Standing{}, false, newErr(CodeUnsupported, "no installed sync backend can use this ticket")
	}
	if string(t.Device) == string(e.ident.Public()) {
		return session.Standing{}, false, session.ErrSelf
	}
	return e.core.Connect(ctx, string(t.Access), identity.DeviceID(t.Device), t.Token)
}

// startPairing registers the operation (or finds the one this RequestID made)
// and runs work in the background. The sensitive parameter is only hashed.
func (e *Engine) startPairing(req RequestID, param string, phase Phase,
	work func(ctx context.Context, op OperationID) (session.Standing, bool, error)) (OperationID, error) {

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
		st, awaiting, err := work(ctx, id)
		if err != nil {
			ee := mapNetErr(err)
			e.updateOp(id, func(o *Operation) { o.Phase, o.Error = PhaseFailed, ee })
			return
		}
		e.reconcile() // the new device is known now; a ready one connects
		peer, _ := e.st.Device(st.DeviceID)
		fp := identity.Fingerprint(e.ident.Public(), peer.PublicKey)
		e.updateOp(id, func(o *Operation) {
			o.Phase = PhaseSucceeded
			o.Pairing = &PairingResult{PeerID: DeviceID(st.DeviceID), Fingerprint: fp,
				AwaitingLocalApproval: !st.Allowed, AwaitingRemoteApproval: !e.core.Connected(st.DeviceID)}
			_ = awaiting
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
