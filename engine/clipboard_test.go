package engine

import (
	"testing"
	"time"
)

var hostCaps = HostCapabilities{
	ReadFormats: []string{MimeText, MimeHTML, MimePNG}, WriteFormats: []string{MimeText, MimeHTML},
	ReadFiles: true, WriteFiles: true, Observe: AbilityAvailable, Apply: AbilityAvailable,
}

func withHost(o *Options) { o.HostCapabilities = hostCaps }

// clipboardHost drives the observation protocol the way a host does.
type clipboardHost struct {
	t     *testing.T
	e     *Engine
	epoch string
	seq   uint64
}

func newHost(t *testing.T, e *Engine) *clipboardHost {
	h := &clipboardHost{t: t, e: e, epoch: "epoch-" + randHex(3)}
	return h
}

// observe imports content (a string; "" is an empty clipboard) and reports it.
func (h *clipboardHost) observe(mode ObserveMode, text string, token ApplyToken) (ObservationResult, error) {
	h.t.Helper()
	var lease LeaseID
	if text != "" {
		hd := importText(h.t, h.e, text)
		lease = hd.LeaseID
		defer func() { _ = h.e.ReleaseContent(bg, lease) }()
	}
	h.seq++
	return h.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: h.seq, Mode: mode, LeaseID: lease, AppliedToken: token})
}

func (h *clipboardHost) must(mode ObserveMode, text string) Disposition {
	h.t.Helper()
	r, err := h.observe(mode, text, "")
	if err != nil {
		h.t.Fatal(err)
	}
	return r.Disposition
}

func TestObservationProtocol(t *testing.T) {
	n := newContentNode(t, withHost)
	h := newHost(t, n.e)
	if _, err := h.observe(ObserveBaseline, "", ""); code(err) != CodeInvalidState {
		t.Fatalf("before Start: %v", err)
	}
	h.seq = 0
	start(t, n)
	l := newLegacy(t, n)

	if d := h.must(ObserveBaseline, "already here"); d != DispBaseline {
		t.Fatalf("baseline = %s", d)
	}
	time.Sleep(100 * time.Millisecond)
	if l.board.writes() != 0 {
		t.Fatal("the baseline was broadcast")
	}
	if d := h.must(ObserveChange, "already here"); d != DispUnchanged {
		t.Fatalf("same content = %s", d)
	}
	r, err := h.observe(ObserveChange, "new copy", "")
	if err != nil || r.Disposition != DispSubmitted || r.OperationID == "" {
		t.Fatalf("change = %+v, %v", r, err)
	}
	if o := waitSend(t, n.e, r.OperationID); o.Outcome != OutcomeSuccess {
		t.Fatalf("send = %+v", o)
	}
	eventually(t, "peer applied", func() bool { return l.board.part(MimeText) == "new copy" })
	if d := h.must(ObserveChange, ""); d != DispEmpty {
		t.Fatalf("cleared = %s", d)
	}
	// Copying the same thing again after clearing is a change again.
	if d := h.must(ObserveChange, "new copy"); d != DispSubmitted {
		t.Fatalf("copy again = %s", d)
	}

	// Sequence rules.
	before := h.seq
	hd := importText(t, n.e, "x")
	defer func() { _ = n.e.ReleaseContent(bg, hd.LeaseID) }()
	first, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before + 1, Mode: ObserveChange, LeaseID: hd.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	before++
	_ = n.e.ReleaseContent(bg, hd.LeaseID) // the lease is gone, the answer is not
	if again, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before, Mode: ObserveChange, LeaseID: hd.LeaseID}); err != nil || again != first {
		t.Fatalf("a repeat of the latest observation must be answered again: %+v, %v", again, err)
	}
	if _, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before, Mode: ObserveChange}); code(err) != CodeConflict {
		t.Fatalf("same sequence, other content: %v", err)
	}
	if _, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before - 1, Mode: ObserveChange, LeaseID: hd.LeaseID}); code(err) != CodeConflict {
		t.Fatalf("old sequence: %v", err)
	}
	if _, err := n.e.ObserveClipboard(bg, Observation{Epoch: "other", Sequence: 1, Mode: ObserveChange, LeaseID: hd.LeaseID}); code(err) != CodeInvalidArgument {
		t.Fatalf("a new epoch must start with a baseline: %v", err)
	}
	if _, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before + 1, Mode: ObserveBaseline}); code(err) != CodeInvalidArgument {
		t.Fatalf("baseline in the middle of an epoch: %v", err)
	}
	if _, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: before + 5, Mode: ObserveChange, LeaseID: LeaseID(n.e.instance) + "-gone"}); code(err) != CodeNotFound {
		t.Fatalf("unknown lease: %v", err)
	}
	// A rejected observation did not advance anything, and gaps are fine.
	h.seq = before + 9
	if d := h.must(ObserveChange, "after a gap"); d != DispSubmitted {
		t.Fatalf("after a gap = %s", d)
	}
}

func TestUnsendableChangesStillMoveTheBaseline(t *testing.T) {
	n := newContentNode(t, withHost)
	start(t, n)
	h := newHost(t, n.e)
	h.must(ObserveBaseline, "")
	// Nobody is connected: nothing is sent now, and nothing later.
	if d := h.must(ObserveChange, "lonely"); d != DispNoTargets {
		t.Fatalf("no peers = %s", d)
	}
	l := newLegacy(t, n)
	if d := h.must(ObserveChange, "lonely"); d != DispUnchanged {
		t.Fatalf("same content once a peer is there = %s", d)
	}
	time.Sleep(100 * time.Millisecond)
	if l.board.writes() != 0 {
		t.Fatal("old content was delivered after the fact")
	}
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = false })
	if d := h.must(ObserveChange, "paused"); d != DispDisabled {
		t.Fatalf("paused = %s", d)
	}
	updateSettings(t, n.e, func(s *Settings) { s.Sync.Enabled = true })
	big := importText(t, n.e, "too big")
	updateSettings(t, n.e, func(s *Settings) { s.Limits.MaxTotalBytes = 3 }) // lowered after the host imported it
	h.seq++
	res, err := n.e.ObserveClipboard(bg, Observation{Epoch: h.epoch, Sequence: h.seq, Mode: ObserveChange, LeaseID: big.LeaseID})
	_ = n.e.ReleaseContent(bg, big.LeaseID)
	if err != nil || res.Disposition != DispTooLarge {
		t.Fatalf("limit = %+v, %v", res, err)
	}
	updateSettings(t, n.e, func(s *Settings) { s.Limits.MaxTotalBytes = 1 << 20 })
	if d := h.must(ObserveChange, "too big"); d != DispUnchanged {
		t.Fatalf("re-enabling must not send what was skipped: %s", d)
	}
}

func TestSuspendEndsTheEpoch(t *testing.T) {
	n := newContentNode(t, withHost)
	start(t, n)
	h := newHost(t, n.e)
	h.must(ObserveBaseline, "")
	_ = n.e.SetRuntimeState(bg, RuntimeSuspended)
	if _, err := h.observe(ObserveChange, "while asleep", ""); code(err) != CodeInvalidArgument && code(err) != CodeSuspended {
		t.Fatalf("change while suspended: %v", err)
	}
	_ = n.e.SetRuntimeState(bg, RuntimeActive)
	// The old epoch is gone: resuming means a new baseline, which does not send
	// whatever was copied in the meantime.
	if _, err := h.observe(ObserveChange, "stale epoch", ""); code(err) != CodeInvalidArgument {
		t.Fatalf("old epoch after resume: %v", err)
	}
	h2 := newHost(t, n.e)
	l := newLegacy(t, n)
	if d := h2.must(ObserveBaseline, "copied while asleep"); d != DispBaseline {
		t.Fatalf("new epoch = %s", d)
	}
	time.Sleep(100 * time.Millisecond)
	if l.board.writes() != 0 {
		t.Fatal("content from the suspended period was sent")
	}
}

// receiveFrom sends text from a to b and returns b's receipt.
func receiveFrom(t *testing.T, a, b node, s string) ReceivedContent {
	t.Helper()
	before := len(received(b.e))
	sendText(t, a, b, s)
	eventually(t, "receipt", func() bool { return len(received(b.e)) == before+1 })
	return received(b.e)[0]
}

func appliedPair(t *testing.T) (a, b node, hb *clipboardHost) {
	a, b = pairedPair(t, withHost)
	hb = newHost(t, b.e)
	hb.must(ObserveBaseline, "")
	return
}

func readParts(t *testing.T, e *Engine, tk ApplyTicket) map[string]string {
	t.Helper()
	out := map[string]string{}
	c, err := e.RetainContent(bg, tk.ContentID)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.ReleaseContent(bg, c.LeaseID) }()
	for _, p := range c.Content.Parts {
		out[p.MIME] = readAll(t, e, c.LeaseID, p.ID)
	}
	return out
}

func TestApplyThenObserveIsNotAnEcho(t *testing.T) {
	a, b, hb := appliedPair(t)
	r := receiveFrom(t, a, b, "from a")

	tk, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
	if err != nil {
		t.Fatal(err)
	}
	if st := state(t, b.e); st.ActiveApply == nil || st.ActiveApply.ReceiptID != r.ReceiptID || st.Received[0].ApplyState != ApplyApplying {
		t.Fatalf("state during apply = %+v / %+v", st.ActiveApply, st.Received)
	}
	if _, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq}); code(err) != CodeInUse {
		t.Fatalf("second apply: %v", err)
	}
	if err := b.e.DismissReceived(bg, r.ReceiptID); code(err) != CodeInUse {
		t.Fatalf("dismissing what is being applied: %v", err)
	}
	if got := readParts(t, b.e, tk); got[MimeText] != "from a" {
		t.Fatalf("ticket content = %v", got)
	}

	// The system adds a representation of its own: the read-back differs from
	// what was sent, and that is what the baseline becomes.
	back, _ := b.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "from a"), rep(MimeHTML, "<p>from a</p>")}})
	hb.seq++
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, BaselineLeaseID: back.LeaseID, Epoch: hb.epoch, Sequence: hb.seq}); err != nil {
		t.Fatal(err)
	}
	_ = b.e.ReleaseContent(bg, back.LeaseID)
	_ = b.e.ReleaseContent(bg, tk.LeaseID)
	if st := state(t, b.e); st.ActiveApply != nil || st.Received[0].ApplyState != ApplyApplied {
		t.Fatalf("state after apply = %+v / %+v", st.ActiveApply, st.Received[0].ApplyState)
	}
	// Repeating the report is harmless; a different one is not.
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, BaselineLeaseID: back.LeaseID, Epoch: hb.epoch, Sequence: hb.seq}); err != nil {
		t.Fatalf("repeat: %v", err)
	}
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeFailed}); code(err) != CodeConflict {
		t.Fatalf("different result: %v", err)
	}

	// The poll then sees the clipboard it just wrote (with the extra format).
	hd, _ := b.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "from a"), rep(MimeHTML, "<p>from a</p>")}})
	hb.seq++
	res, err := b.e.ObserveClipboard(bg, Observation{Epoch: hb.epoch, Sequence: hb.seq, Mode: ObserveChange, LeaseID: hd.LeaseID})
	_ = b.e.ReleaseContent(bg, hd.LeaseID)
	if err != nil || res.Disposition != DispUnchanged {
		t.Fatalf("observing the written clipboard = %+v, %v", res, err)
	}
	if ops := state(t, b.e).Operations; countKind(ops, OperationSend) != 0 {
		t.Fatal("the applied content was sent back")
	}
	// A genuinely new copy still goes out.
	if d := hb.must(ObserveChange, "mine"); d != DispSubmitted {
		t.Fatalf("new copy = %s", d)
	}
}

func countKind(ops []Operation, k OperationKind) (n int) {
	for _, o := range ops {
		if o.Kind == k {
			n++
		}
	}
	return
}

func TestEchoByTokenAndLocalCopyDuringApply(t *testing.T) {
	a, b, hb := appliedPair(t)
	r := receiveFrom(t, a, b, "remote")
	tk, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
	if err != nil {
		t.Fatal(err)
	}
	// An observation of our own write, marked with the token, is an echo.
	res, err := hb.observe(ObserveChange, "remote", tk.Token)
	if err != nil || res.Disposition != DispEcho {
		t.Fatalf("echo = %+v, %v", res, err)
	}
	if _, err := hb.observe(ObserveChange, "remote", "not-the-token"); code(err) != CodeConflict {
		t.Fatalf("a stale token: %v", err)
	}
	// Then the user copies something else before the host reports.
	if d := hb.must(ObserveChange, "user copy"); d != DispSubmitted {
		t.Fatalf("user copy = %s (a local change during an apply is a real change)", d)
	}
	back, _ := b.e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, "remote")}})
	lastAccepted := hb.seq
	hb.seq++
	err = b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, BaselineLeaseID: back.LeaseID, Epoch: hb.epoch, Sequence: hb.seq})
	if code(err) != CodeConflict {
		t.Fatalf("a success report after a local copy: %v", err)
	}
	// The slot is free again and the receipt can be applied once more.
	if st := state(t, b.e); st.ActiveApply != nil || st.Received[0].ApplyState != ApplyFailed {
		t.Fatalf("after the conflict: %+v %v", st.ActiveApply, st.Received[0].ApplyState)
	}
	if _, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: lastAccepted}); err != nil {
		t.Fatalf("applying again after the conflict: %v", err)
	}
}

func TestBeginApplyChecks(t *testing.T) {
	a, b, hb := appliedPair(t)
	r := receiveFrom(t, a, b, "x")
	req := ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq}

	stale := req
	stale.ExpectedSequence--
	if _, err := b.e.BeginApply(bg, stale); code(err) != CodeConflict {
		t.Fatalf("stale observation: %v", err)
	}
	if _, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: "nope", ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq}); code(err) != CodeNotFound {
		t.Fatalf("unknown receipt: %v", err)
	}
	// The user copies something: the observation the host quoted is now stale.
	hb.must(ObserveChange, "newer")
	if _, err := b.e.BeginApply(bg, req); code(err) != CodeConflict {
		t.Fatalf("applying over a newer copy: %v", err)
	}
	req.ExpectedSequence = hb.seq

	caps := hostCaps
	caps.WriteFormats = []string{MimePNG} // cannot write what was received
	_ = b.e.UpdateHostCapabilities(bg, caps)
	if _, err := b.e.BeginApply(bg, req); code(err) != CodeUnsupported {
		t.Fatalf("nothing writable: %v", err)
	}
	caps = hostCaps
	caps.Apply = AbilityPermissionRequired
	_ = b.e.UpdateHostCapabilities(bg, caps)
	if _, err := b.e.BeginApply(bg, req); code(err) != CodePermissionDenied {
		t.Fatalf("no permission: %v", err)
	}
	caps.Apply = AbilityUnsupported
	_ = b.e.UpdateHostCapabilities(bg, caps)
	if _, err := b.e.BeginApply(bg, req); code(err) != CodeUnsupported {
		t.Fatalf("unsupported: %v", err)
	}
	_ = b.e.UpdateHostCapabilities(bg, hostCaps)
	updateSettings(t, b.e, func(s *Settings) { s.Sync.Text = false })
	if _, err := b.e.BeginApply(bg, req); code(err) != CodeUnsupported {
		t.Fatalf("text disabled: %v", err)
	}
	updateSettings(t, b.e, func(s *Settings) { s.Sync.Enabled = false })
	if _, err := b.e.BeginApply(bg, req); code(err) != CodeSyncDisabled {
		t.Fatalf("paused: %v", err)
	}
}

func TestFailedAndDeferredKeepTheBaseline(t *testing.T) {
	a, b, hb := appliedPair(t)
	r := receiveFrom(t, a, b, "x")
	for _, out := range []ApplyOutcome{ApplyOutcomeFailed, ApplyOutcomeDeferred} {
		tk, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
		if err != nil {
			t.Fatalf("%s: %v", out, err)
		}
		if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: out, ErrorCode: "write_failed"}); err != nil {
			t.Fatal(err)
		}
		_ = b.e.ReleaseContent(bg, tk.LeaseID)
		want := ApplyFailed
		if out == ApplyOutcomeDeferred {
			want = ApplyDeferred
		}
		if got := received(b.e)[0].ApplyState; got != want {
			t.Fatalf("%s: state %s", out, got)
		}
	}
	// The observation numbers did not move, so the next observation continues.
	if d := hb.must(ObserveChange, "local"); d != DispSubmitted {
		t.Fatalf("after failures = %s", d)
	}
	// Unknown or unrelated tokens are conflicts.
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: "bogus", Outcome: ApplyOutcomeFailed}); code(err) != CodeConflict {
		t.Fatalf("bogus token: %v", err)
	}
}

func TestSuspendVoidsTheTicket(t *testing.T) {
	a, b, hb := appliedPair(t)
	r := receiveFrom(t, a, b, "x")
	tk, _ := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
	_ = b.e.SetRuntimeState(bg, RuntimeSuspended)
	_ = b.e.SetRuntimeState(bg, RuntimeActive)
	// The slot stays taken until the host says how it ended; success is refused.
	st := state(t, b.e)
	if st.ActiveApply == nil || len(st.Received) == 0 || st.Received[0].ApplyState == ApplyApplying {
		t.Fatalf("after suspend: %+v", st)
	}
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied, Epoch: hb.epoch, Sequence: hb.seq + 1}); code(err) != CodeConflict {
		t.Fatalf("success of a voided ticket: %v", err)
	}
	if st := state(t, b.e); st.ActiveApply != nil {
		t.Fatal("the slot was not freed by the host's report")
	}
	_ = b.e.ReleaseContent(bg, tk.LeaseID)
}

func TestApplyWithoutObservation(t *testing.T) {
	// A host that cannot watch the clipboard (a phone in the background) can
	// still apply what it received.
	a, b := pairedPair(t, func(o *Options) {
		o.HostCapabilities = hostCaps
		o.HostCapabilities.Observe = AbilityUnsupported
	})
	r := receiveFrom(t, a, b, "hello phone")
	tk, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID})
	if err != nil {
		t.Fatal(err)
	}
	if err := b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeApplied}); err != nil {
		t.Fatal(err)
	}
	_ = b.e.ReleaseContent(bg, tk.LeaseID)
	if received(b.e)[0].ApplyState != ApplyApplied {
		t.Fatal("not marked applied")
	}
}

func TestReceiptInUseIsNotEvictedOrExpired(t *testing.T) {
	a, b := pairedPair(t, func(o *Options) {
		withHost(o)
		o.Resources.MaxReceived = 1
		o.Resources.ReceivedTTL = 500 * time.Millisecond
	})
	hb := newHost(t, b.e)
	hb.must(ObserveBaseline, "")
	r := receiveFrom(t, a, b, "being applied")
	tk, err := b.e.BeginApply(bg, ApplyRequest{ReceiptID: r.ReceiptID, ExpectedEpoch: hb.epoch, ExpectedSequence: hb.seq})
	if err != nil {
		t.Fatal(err)
	}
	// The list is full and its only entry is in use: the newcomer loses.
	sendText(t, a, b, "newcomer")
	time.Sleep(700 * time.Millisecond) // past the TTL
	rs := received(b.e)
	if len(rs) != 1 || rs[0].ReceiptID != r.ReceiptID {
		t.Fatalf("list = %+v", rs)
	}
	_ = b.e.ReportApplyResult(bg, ApplyResult{Token: tk.Token, Outcome: ApplyOutcomeFailed})
	_ = b.e.ReleaseContent(bg, tk.LeaseID)
	// Released, it expires normally.
	eventually(t, "expiry after the application ends", func() bool { return len(received(b.e)) == 0 })
}
