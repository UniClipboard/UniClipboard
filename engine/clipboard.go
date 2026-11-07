package engine

import (
	"context"
	"crypto/sha256"
	"strconv"
	"time"

	"github.com/uniclipboard/unic/internal/clip"
)

// ---- DTOs ----

// ObserveMode is what an observation reports.
type ObserveMode string

const (
	ObserveBaseline ObserveMode = "baseline" // the clipboard as it is; nothing is sent
	ObserveChange   ObserveMode = "change"   // the clipboard changed
)

// Observation reports the host's clipboard to the engine. A host starts
// observing with a fresh random Epoch and a baseline at Sequence 1, then
// counts up. LeaseID leases the current clipboard content (ImportContent);
// empty means an empty clipboard. The host releases the lease afterwards.
type Observation struct {
	Epoch    string
	Sequence uint64
	Mode     ObserveMode
	LeaseID  LeaseID
	// AppliedToken marks an observation the host knows is the echo of its own
	// write for that apply ticket. Never set it just because the time fits.
	AppliedToken ApplyToken
}

// Disposition tells what the engine did with an observation.
type Disposition string

const (
	DispBaseline  Disposition = "baseline"
	DispUnchanged Disposition = "unchanged" // same content as the baseline
	DispEmpty     Disposition = "empty"     // the clipboard was cleared
	DispEcho      Disposition = "echo"      // the write of a remote content
	DispSubmitted Disposition = "submitted"
	DispDisabled  Disposition = "disabled" // sync is paused or this content is not enabled
	DispNoTargets Disposition = "no_targets"
	DispTooLarge  Disposition = "too_large" // over the configured limits
)

// ObservationResult is the answer to an observation. Every disposition other
// than an error advances the baseline: content that could not be sent is not
// sent later either.
type ObservationResult struct {
	Disposition Disposition
	OperationID OperationID // set for DispSubmitted
}

// ApplyToken identifies one application of received content.
type ApplyToken string

// ApplyRequest asks to put received content to use. ExpectedEpoch and
// ExpectedSequence are the host's latest observation: if the clipboard moved
// on since, the request conflicts instead of overwriting a newer copy.
type ApplyRequest struct {
	ReceiptID        ReceiptID
	ExpectedEpoch    string
	ExpectedSequence uint64
}

// ApplyTicket entitles the host to write the content. LeaseID keeps it
// readable (and its files in place) until the host releases it; a host that
// put files on the clipboard keeps the lease until they are replaced.
type ApplyTicket struct {
	Token     ApplyToken
	ContentID ContentID
	LeaseID   LeaseID
}

// ApplyOutcome is how an application ended.
type ApplyOutcome string

const (
	ApplyOutcomeApplied  ApplyOutcome = "applied"
	ApplyOutcomeFailed   ApplyOutcome = "failed"
	ApplyOutcomeDeferred ApplyOutcome = "deferred" // the host will try later (needs a fresh ticket)
)

// ApplyResult closes a ticket. For applied, BaselineLeaseID leases what the
// host read back from the clipboard after writing (empty if it reads empty),
// and Epoch/Sequence is the observation number this baseline takes.
type ApplyResult struct {
	Token           ApplyToken
	Outcome         ApplyOutcome
	BaselineLeaseID LeaseID
	Epoch           string
	Sequence        uint64
	ErrorCode       string // a stable code from the host, for failed; no platform text
}

// ---- state ----

type observation struct {
	epoch    string
	seq      uint64
	baseline string // content id of the clipboard as last seen; "" is empty
	last     *obsRecord
}

type obsRecord struct {
	seq    uint64
	params [sha256.Size]byte
	result ObservationResult
}

type applyTicket struct {
	token   ApplyToken
	receipt ReceiptID
	content ContentID
	lease   LeaseID
	rc      *receipt
	dirty   bool // a local change happened since BeginApply
	dead    bool // suspended, stopped, or the abilities went away
}

type applyDone struct {
	token  ApplyToken
	params [sha256.Size]byte
}

// contentKey identifies content for de-duplication: equal content gives an
// equal key on every device. Files count by name and size only.
func contentKey(c *contentObj) string {
	if c == nil {
		return ""
	}
	var snap clip.Snapshot
	for _, p := range c.parts {
		if p.desc.Kind == PartFile {
			snap.Files = append(snap.Files, clip.File{Name: p.desc.Name, Size: p.desc.Size})
		} else {
			snap.Parts = append(snap.Parts, clip.Part{Format: p.desc.MIME, Data: p.data})
		}
	}
	return snap.Hash()
}

// contentOf finds the content of a lease; an empty lease is an empty clipboard.
func (e *Engine) contentOf(lease LeaseID) (*contentObj, *Error) {
	if lease == "" {
		return nil, nil
	}
	e.cs.mu.Lock()
	defer e.cs.mu.Unlock()
	l := e.cs.leases[lease]
	if l == nil {
		if e.foreign(string(lease)) || e.cs.gone.has(string(lease)) {
			return nil, newErr(CodeResourceExpired, "the lease is no longer valid")
		}
		return nil, newErr(CodeNotFound, "no such lease")
	}
	return l.c, nil
}

// newLease gives the caller a lease on c, which must be alive.
func (e *Engine) newLease(c *contentObj) LeaseID {
	e.cs.mu.Lock()
	defer e.cs.mu.Unlock()
	c.refs++
	l := LeaseID(e.newID())
	e.cs.leases[l] = &leaseRec{c: c}
	return l
}

// ---- observation ----

// ObserveClipboard tells the engine what the host's clipboard holds. See
// Observation for the protocol. Changes are sent (or not) according to the
// sync settings; the baseline moves in every case that is not an error.
func (e *Engine) ObserveClipboard(ctx context.Context, o Observation) (ObservationResult, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ObservationResult{}, gerr
	}
	defer release()
	if o.Epoch == "" || len(o.Epoch) > 64 || o.Sequence == 0 || (o.Mode != ObserveBaseline && o.Mode != ObserveChange) ||
		(o.AppliedToken != "" && o.Mode != ObserveChange) {
		return ObservationResult{}, newErr(CodeInvalidArgument, "invalid observation")
	}
	e.obsMu.Lock()
	defer e.obsMu.Unlock()
	params := sha256.Sum256([]byte(string(o.Mode) + "\x00" + string(o.LeaseID) + "\x00" + string(o.AppliedToken)))

	e.mu.Lock()
	ob := &e.obs
	// A repeat of the latest observation gets its answer again, before the
	// lease is looked at: the lease may be gone by now.
	if ob.epoch == o.Epoch && ob.last != nil && ob.last.seq == o.Sequence {
		defer e.mu.Unlock()
		if ob.last.params != params {
			return ObservationResult{}, newErr(CodeConflict, "that sequence number was used for another observation")
		}
		return ob.last.result, nil
	}
	newEpoch := ob.epoch != o.Epoch
	switch {
	case newEpoch && (o.Mode != ObserveBaseline || o.Sequence != 1):
		e.mu.Unlock()
		return ObservationResult{}, newErr(CodeInvalidArgument, "a new epoch starts with a baseline at sequence 1")
	case !newEpoch && o.Sequence <= ob.seq:
		e.mu.Unlock()
		return ObservationResult{}, newErr(CodeConflict, "stale sequence number")
	case !newEpoch && o.Mode == ObserveBaseline:
		e.mu.Unlock()
		return ObservationResult{}, newErr(CodeInvalidArgument, "only the first observation of an epoch is a baseline")
	}
	life, suspended := e.life, e.runtime == RuntimeSuspended
	e.mu.Unlock()
	if life != LifecycleRunning {
		return ObservationResult{}, newErr(CodeInvalidState, "the engine is not running")
	}

	c, lerr := e.contentOf(o.LeaseID)
	if lerr != nil {
		return ObservationResult{}, lerr
	}
	key := contentKey(c)

	finish := func(res ObservationResult) (ObservationResult, error) {
		e.mu.Lock()
		e.obs.epoch, e.obs.seq, e.obs.baseline = o.Epoch, o.Sequence, key
		e.obs.last = &obsRecord{seq: o.Sequence, params: params, result: res}
		e.mu.Unlock()
		return res, nil
	}

	if o.Mode == ObserveBaseline {
		e.mu.Lock() // a new epoch ends the old one's business
		e.killTicketLocked()
		e.mu.Unlock()
		return finish(ObservationResult{Disposition: DispBaseline})
	}
	if suspended {
		return ObservationResult{}, newErr(CodeSuspended, "the host suspended networking; start a new epoch when resuming")
	}

	// The echo of our own write.
	if o.AppliedToken != "" {
		e.mu.Lock()
		t := e.ticket
		ok := t != nil && t.token == o.AppliedToken && !t.dead
		e.mu.Unlock()
		if !ok {
			return ObservationResult{}, newErr(CodeConflict, "that apply token is not current")
		}
		return finish(ObservationResult{Disposition: DispEcho})
	}
	e.mu.Lock()
	same := key == e.obs.baseline
	e.mu.Unlock()
	if same {
		return finish(ObservationResult{Disposition: DispUnchanged})
	}

	// A real local change. It overrides an application in progress: the
	// host's write may already have been overwritten.
	e.mu.Lock()
	if e.ticket != nil {
		e.ticket.dirty = true
	}
	e.mu.Unlock()
	if c == nil {
		return finish(ObservationResult{Disposition: DispEmpty})
	}
	id, err := e.submit(SubmitRequest{RequestID: RequestID("obs:" + o.Epoch + ":" + strconv.FormatUint(o.Sequence, 10)), LeaseID: o.LeaseID, Origin: OriginClipboard})
	switch codeOf(err) {
	case "":
		return finish(ObservationResult{Disposition: DispSubmitted, OperationID: id})
	case CodeNoTargets:
		return finish(ObservationResult{Disposition: DispNoTargets})
	case CodeSyncDisabled:
		return finish(ObservationResult{Disposition: DispDisabled})
	case CodeLimitExceeded:
		return finish(ObservationResult{Disposition: DispTooLarge})
	}
	return ObservationResult{}, err // not advanced: the same change can be reported again
}

func codeOf(err error) Code {
	if err == nil {
		return ""
	}
	return asError(err).Code
}

// ---- application ----

// killTicketLocked marks the application in progress as void and frees its
// receipt. The host still has to report an outcome to free the slot. e.mu held.
func (e *Engine) killTicketLocked() {
	t := e.ticket
	if t == nil || t.dead {
		return
	}
	t.dead = true
	if t.rc != nil {
		t.rc.locked, t.rc.ApplyState = false, ApplyAvailable
	}
	e.commitLocked(EventApply, EventReceived)
	e.rescheduleRecvLocked()
}

// BeginApply starts putting received content on the host's clipboard. It
// checks the receipt, the sync settings, the host's abilities and that the
// clipboard did not move on since the host's last observation. Only one
// application can be under way; the receipt cannot be evicted meanwhile.
func (e *Engine) BeginApply(ctx context.Context, req ApplyRequest) (ApplyTicket, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return ApplyTicket{}, gerr
	}
	defer release()
	e.obsMu.Lock()
	defer e.obsMu.Unlock()
	e.mu.Lock()
	defer e.mu.Unlock()

	if e.life != LifecycleRunning {
		return ApplyTicket{}, newErr(CodeInvalidState, "the engine is not running")
	}
	if e.runtime == RuntimeSuspended {
		return ApplyTicket{}, newErr(CodeSuspended, "the host suspended networking")
	}
	switch e.host.Apply {
	case AbilityAvailable:
	case AbilityPermissionRequired:
		return ApplyTicket{}, newErr(CodePermissionDenied, "the system denies writing the clipboard")
	case AbilityUnavailable:
		return ApplyTicket{}, retryable(newErr(CodeUnavailable, "the host cannot write the clipboard now"))
	default:
		return ApplyTicket{}, newErr(CodeUnsupported, "the host cannot write the clipboard")
	}
	sy := e.settings.Value.Sync
	if !sy.Enabled {
		return ApplyTicket{}, newErr(CodeSyncDisabled, "sync is paused")
	}
	if e.ticket != nil {
		return ApplyTicket{}, newErr(CodeInUse, "another application is under way")
	}
	if req.ExpectedEpoch != e.obs.epoch || (req.ExpectedEpoch != "" && req.ExpectedSequence != e.obs.seq) {
		return ApplyTicket{}, newErr(CodeConflict, "the clipboard changed since the last observation")
	}
	var rc *receipt
	for _, r := range e.recv {
		if r.ReceiptID == req.ReceiptID && time.Now().Before(r.expires) {
			rc = r
		}
	}
	if rc == nil {
		return ApplyTicket{}, newErr(CodeNotFound, "no such received content (it may have expired)")
	}
	if !e.appliable(rc.c, sy) {
		return ApplyTicket{}, newErr(CodeUnsupported, "nothing in this content can be applied here")
	}
	t := &applyTicket{token: ApplyToken(e.newID()), receipt: rc.ReceiptID, content: rc.c.id, rc: rc}
	t.lease = e.newLease(rc.c)
	rc.locked, rc.ApplyState = true, ApplyApplying
	e.ticket = t
	e.commitLocked(EventApply, EventReceived)
	return ApplyTicket{Token: t.token, ContentID: t.content, LeaseID: t.lease}, nil
}

// appliable reports whether some part can be written by this host under the
// current sync settings. e.mu held.
func (e *Engine) appliable(c *contentObj, sy SyncSettings) bool {
	for _, p := range c.parts {
		switch {
		case p.desc.Kind == PartFile:
			if sy.Files && e.host.WriteFiles {
				return true
			}
		case p.desc.MIME == MimePNG:
			if sy.Images && contains(e.host.WriteFormats, p.desc.MIME) {
				return true
			}
		default:
			if sy.Text && contains(e.host.WriteFormats, p.desc.MIME) {
				return true
			}
		}
	}
	return false
}

// ReportApplyResult closes an application. Applied moves the observation
// baseline to what the host read back, so the write is not mistaken for a
// local copy. It conflicts if the clipboard changed meanwhile (the host then
// re-reads the clipboard and observes it as it is) or if the engine voided the
// ticket (suspend, stop, abilities gone). Failed and deferred never move the
// baseline. Reporting the same result again succeeds.
func (e *Engine) ReportApplyResult(ctx context.Context, r ApplyResult) error {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return gerr
	}
	defer release()
	switch r.Outcome {
	case ApplyOutcomeApplied, ApplyOutcomeFailed, ApplyOutcomeDeferred:
	default:
		return newErr(CodeInvalidArgument, "unknown outcome")
	}
	if r.Token == "" || len(r.ErrorCode) > 64 {
		return newErr(CodeInvalidArgument, "invalid apply result")
	}
	e.obsMu.Lock()
	defer e.obsMu.Unlock()
	params := sha256.Sum256([]byte(string(r.Outcome) + "\x00" + string(r.BaselineLeaseID) + "\x00" + r.Epoch + "\x00" +
		strconv.FormatUint(r.Sequence, 10) + "\x00" + r.ErrorCode))

	e.mu.Lock()
	t := e.ticket
	if t == nil || t.token != r.Token {
		defer e.mu.Unlock()
		if e.lastApply.token == r.Token && e.lastApply.params == params {
			return nil
		}
		return newErr(CodeConflict, "that apply token is not current")
	}
	e.mu.Unlock()

	var key string
	if r.Outcome == ApplyOutcomeApplied {
		c, lerr := e.contentOf(r.BaselineLeaseID)
		if lerr != nil {
			return lerr
		}
		key = contentKey(c)
	}

	e.mu.Lock()
	defer e.mu.Unlock()
	finish := func(state ApplyState) {
		if t.rc != nil && !t.dead {
			t.rc.locked, t.rc.ApplyState = false, state
		}
		e.ticket = nil
		e.lastApply = applyDone{token: t.token, params: params}
		e.commitLocked(EventApply, EventReceived)
		e.rescheduleRecvLocked()
	}
	switch r.Outcome {
	case ApplyOutcomeFailed:
		finish(ApplyFailed)
	case ApplyOutcomeDeferred:
		finish(ApplyDeferred)
	default:
		// A host that does not observe the clipboard has no baseline to move.
		observing := e.obs.epoch != ""
		if t.dirty || t.dead || r.Epoch != e.obs.epoch || (observing && r.Sequence <= e.obs.seq) {
			finish(ApplyFailed)
			return newErr(CodeConflict, "the clipboard changed while the content was being applied")
		}
		if observing {
			e.obs.seq, e.obs.baseline, e.obs.last = r.Sequence, key, nil
		}
		finish(ApplyApplied)
	}
	return nil
}
