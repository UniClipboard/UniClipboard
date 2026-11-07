package engine

import (
	"bytes"
	"context"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

// remote is a second engine, paired with the one under test, that receives
// what it sends.
type remote struct{ node }

// newLegacy starts a second engine and pairs it with a, as two users would.
func newLegacy(t *testing.T, a node) *remote {
	t.Helper()
	b := newContentNode(t, nil)
	start(t, b)
	pairTwo(t, a, b)
	return &remote{b}
}

// count is the number of clips received so far (the list is long enough here).
func (r *remote) count() int { return len(received(r.e)) }

// text is the text of the newest clip received.
func (r *remote) text() string {
	rs := received(r.e)
	if len(rs) == 0 {
		return ""
	}
	return textOfE(r.e, rs[0])
}

func senderNode(t *testing.T, mutate func(*Options)) (node, *remote) {
	t.Helper()
	n := newContentNode(t, mutate)
	start(t, n)
	return n, newLegacy(t, n)
}

func importText(t *testing.T, e *Engine, s string) ContentHandle {
	t.Helper()
	h, err := e.ImportContent(bg, ContentInput{Representations: []RepresentationInput{rep(MimeText, s)}})
	if err != nil {
		t.Fatal(err)
	}
	return h
}

var reqN atomic.Int64

func submit(e *Engine, h ContentHandle) (OperationID, error) {
	return e.SubmitContent(bg, SubmitRequest{RequestID: RequestID(fmt.Sprint("r", reqN.Add(1))), LeaseID: h.LeaseID, Origin: OriginExplicit})
}

func waitSend(t *testing.T, e *Engine, id OperationID) Operation {
	t.Helper()
	var o Operation
	eventually(t, "send to complete", func() bool {
		var err error
		if o, err = e.GetOperation(bg, id); err != nil {
			t.Fatal(err)
		}
		return o.Done()
	})
	return o
}

func TestSubmitDeliversEverything(t *testing.T) {
	n, l := senderNode(t, nil)
	img := pngBytes(t)
	ref := filepath.Join(t.TempDir(), "ref.bin")
	refData := strings.Repeat("r", 100_000)
	if err := os.WriteFile(ref, []byte(refData), 0o600); err != nil {
		t.Fatal(err)
	}

	h, err := n.e.ImportContent(bg, ContentInput{
		Representations: []RepresentationInput{rep(MimeText, "héllo"), rep(MimeHTML, "<i>x</i>"),
			{MIME: MimePNG, Size: int64(len(img)), Reader: bytes.NewReader(img)}},
		Files: []FileInput{fileIn("copied.txt", "copied bytes"), {Name: "ref.bin", Size: int64(len(refData)), Source: pathSource{ref}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	id, err := submit(n.e, h)
	if err != nil {
		t.Fatal(err)
	}
	o := waitSend(t, n.e, id)
	if o.Kind != OperationSend || o.Phase != PhaseCompleted || o.Outcome != OutcomeSuccess || len(o.Targets) != 1 || o.Targets[0].State != TargetSent {
		t.Fatalf("operation = %+v", o)
	}
	eventually(t, "the peer has it", func() bool { return l.count() == 1 })
	reps, files := contentOf(t, l.e, received(l.e)[0])
	if reps[MimeText] != "héllo" || reps[MimeHTML] != "<i>x</i>" || reps[MimePNG] != string(img) {
		t.Fatalf("representations differ: %d", len(reps))
	}
	if files["copied.txt"] != "copied bytes" || files["ref.bin"] != refData || len(files) != 2 {
		t.Fatalf("files differ: %v", len(files))
	}
}

func TestSubmitNeedsAnOnlineTargetAndSendsNothingLater(t *testing.T) {
	n := newContentNode(t, nil)
	start(t, n)
	h := importText(t, n.e, "nobody home")
	if _, err := submit(n.e, h); code(err) != CodeNoTargets {
		t.Fatalf("no peers: %v", err)
	}
	if ops := state(t, n.e).Operations; len(ops) != 0 {
		t.Fatalf("a refused submit left an operation: %+v", ops)
	}
	// The failed submit holds nothing: releasing the lease frees the content.
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	if len(n.e.cs.contents) != 0 {
		t.Fatal("a refused submit leaked a reference")
	}
	// A device that connects afterwards gets nothing.
	l := newLegacy(t, n)
	time.Sleep(200 * time.Millisecond)
	if l.count() != 0 {
		t.Fatal("old content was delivered after the fact")
	}
}

func TestSubmitValidationAndIdempotency(t *testing.T) {
	n, l := senderNode(t, nil)
	h := importText(t, n.e, "once")
	req := SubmitRequest{RequestID: "same", LeaseID: h.LeaseID, Origin: OriginExplicit}

	if _, err := n.e.SubmitContent(bg, SubmitRequest{LeaseID: h.LeaseID, Origin: OriginExplicit}); code(err) != CodeInvalidArgument {
		t.Fatalf("no RequestID: %v", err)
	}
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "c", LeaseID: h.LeaseID, Origin: OriginClipboard}); code(err) != CodeInvalidArgument {
		t.Fatalf("clipboard origin from outside: %v", err)
	}
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "u", LeaseID: LeaseID(n.e.instance) + "-nope", Origin: OriginExplicit}); code(err) != CodeNotFound {
		t.Fatalf("unknown lease: %v", err)
	}
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "u2", LeaseID: "0000000000000000-x", Origin: OriginExplicit}); code(err) != CodeResourceExpired {
		t.Fatalf("foreign lease: %v", err)
	}
	id, err := n.e.SubmitContent(bg, req)
	if err != nil {
		t.Fatal(err)
	}
	waitSend(t, n.e, id)
	// The same request is answered again without sending again, even after the
	// lease is gone; a different request under the same id is a conflict.
	_ = n.e.ReleaseContent(bg, h.LeaseID)
	if again, err := n.e.SubmitContent(bg, req); err != nil || again != id {
		t.Fatalf("repeat = %v, %v", again, err)
	}
	h2 := importText(t, n.e, "other")
	if _, err := n.e.SubmitContent(bg, SubmitRequest{RequestID: "same", LeaseID: h2.LeaseID, Origin: OriginExplicit}); code(err) != CodeConflict {
		t.Fatalf("reused RequestID: %v", err)
	}
	time.Sleep(100 * time.Millisecond)
	if l.count() != 1 {
		t.Fatalf("the peer applied %d clips, want 1", l.count())
	}
	// Identical content may be sent again on request: explicit sends are not de-duplicated.
	h3 := importText(t, n.e, "once")
	id3, err := submit(n.e, h3)
	if err != nil {
		t.Fatal(err)
	}
	waitSend(t, n.e, id3)
}

func TestSubmitFollowsSyncSettings(t *testing.T) {
	n, l := senderNode(t, nil)
	two := func() ContentInput {
		return ContentInput{Representations: []RepresentationInput{rep(MimeText, "words")}, Files: []FileInput{fileIn("f.txt", "file")}}
	}
	set := func(f func(*Settings)) {
		s := state(t, n.e).Settings
		f(&s.Value)
		if _, err := n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value}); err != nil {
			t.Fatal(err)
		}
	}
	// Text off: only the file goes.
	set(func(s *Settings) { s.Sync.Text = false })
	h, _ := n.e.ImportContent(bg, two())
	id, err := submit(n.e, h)
	if err != nil {
		t.Fatal(err)
	}
	waitSend(t, n.e, id)
	eventually(t, "file arrives", func() bool { return l.count() == 1 })
	if reps, files := contentOf(t, l.e, received(l.e)[0]); len(reps) != 0 || len(files) != 1 {
		t.Fatalf("filter ignored: %d representations, %d files", len(reps), len(files))
	}
	// Nothing enabled for this content, or everything paused: refused.
	set(func(s *Settings) { s.Sync.Files = false })
	h, _ = n.e.ImportContent(bg, two())
	if _, err := submit(n.e, h); code(err) != CodeSyncDisabled {
		t.Fatalf("nothing enabled: %v", err)
	}
	set(func(s *Settings) { s.Sync = SyncSettings{Enabled: false, Text: true, Images: true, Files: true} })
	if _, err := submit(n.e, h); code(err) != CodeSyncDisabled {
		t.Fatalf("paused: %v", err)
	}
}

func TestLimitsAreCheckedAtSubmit(t *testing.T) {
	n, l := senderNode(t, nil)
	h, _ := n.e.ImportContent(bg, ContentInput{Files: []FileInput{fileIn("f", "0123456789")}})
	s := state(t, n.e).Settings
	s.Value.Limits.MaxFileBytes = 5
	_, _ = n.e.UpdateSettings(bg, UpdateSettingsRequest{ExpectedRevision: s.Revision, Value: s.Value})
	if _, err := submit(n.e, h); code(err) != CodeLimitExceeded {
		t.Fatalf("over the limit set after import: %v", err)
	}
	if l.count() != 0 {
		t.Fatal("sent over the limit")
	}
}

func TestChangedSourceFailsOnlyThatSend(t *testing.T) {
	n, l := senderNode(t, nil)
	path := filepath.Join(t.TempDir(), "doc")
	if err := os.WriteFile(path, []byte("version one"), 0o600); err != nil {
		t.Fatal(err)
	}
	h, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "doc", Size: 11, Source: pathSource{path}}}})
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte("VERSION TWO"), 0o600); err != nil {
		t.Fatal(err)
	}
	later := time.Now().Add(3 * time.Second)
	if err := os.Chtimes(path, later, later); err != nil {
		t.Fatal(err)
	}
	id, err := submit(n.e, h)
	if err != nil {
		t.Fatal(err)
	}
	o := waitSend(t, n.e, id)
	if o.Outcome != OutcomeFailed || o.Targets[0].Error == nil || o.Targets[0].Error.Code != CodeSourceChanged {
		t.Fatalf("operation = %+v", o)
	}
	if l.count() != 0 {
		t.Fatal("changed bytes were sent")
	}
	// The connection is fine and the next send works.
	id2, err := submit(n.e, importText(t, n.e, "next"))
	if err != nil {
		t.Fatal(err)
	}
	if o := waitSend(t, n.e, id2); o.Outcome != OutcomeSuccess {
		t.Fatalf("next send = %+v", o)
	}
}

// gated is a referenced file whose Open can be made to wait.
type gated struct {
	pathSource
	gate atomic.Pointer[chan struct{}]
}

func (g *gated) Open(ctx context.Context) (io.ReadCloser, FileMeta, error) {
	if c := g.gate.Load(); c != nil {
		select {
		case <-*c:
		case <-ctx.Done():
			return nil, FileMeta{}, ctx.Err()
		}
	}
	return g.pathSource.Open(ctx)
}

func gatedFile(t *testing.T, n node) (*gated, ContentHandle) {
	path := filepath.Join(t.TempDir(), "slow")
	if err := os.WriteFile(path, []byte("slow data"), 0o600); err != nil {
		t.Fatal(err)
	}
	g := &gated{pathSource: pathSource{path}}
	h, err := n.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "slow", Size: 9, Source: g}}})
	if err != nil {
		t.Fatal(err)
	}
	return g, h
}

func targetState(t *testing.T, e *Engine, id OperationID) TargetState {
	o, err := e.GetOperation(bg, id)
	if err != nil {
		t.Fatal(err)
	}
	return o.Targets[0].State
}

func TestNewerContentSupersedesAWaitingOne(t *testing.T) {
	n, l := senderNode(t, nil)
	g, slow := gatedFile(t, n)
	gate := make(chan struct{})
	g.gate.Store(&gate)

	a, _ := submit(n.e, slow) // blocks inside the transfer
	eventually(t, "A sending", func() bool { return targetState(t, n.e, a) == TargetSending })
	b, _ := submit(n.e, importText(t, n.e, "B")) // waits behind A
	if st := targetState(t, n.e, b); st != TargetQueued {
		t.Fatalf("B = %s", st)
	}
	c, _ := submit(n.e, importText(t, n.e, "C")) // replaces B
	eventually(t, "B superseded", func() bool { return targetState(t, n.e, b) == TargetSuperseded })
	ob, _ := n.e.GetOperation(bg, b)
	if ob.Phase != PhaseCompleted || ob.Outcome != OutcomeSuperseded {
		t.Fatalf("B = %+v", ob)
	}

	close(gate)
	if o := waitSend(t, n.e, a); o.Outcome != OutcomeSuccess {
		t.Fatalf("A = %+v", o)
	}
	if o := waitSend(t, n.e, c); o.Outcome != OutcomeSuccess {
		t.Fatalf("C = %+v", o)
	}
	eventually(t, "two clips applied", func() bool { return l.count() == 2 })
	if l.text() != "C" {
		t.Fatalf("the peer ended with %q, want C", l.text())
	}
}

func TestSuspendFailsWaitingAndRunningSends(t *testing.T) {
	n, l := senderNode(t, nil)
	g, slow := gatedFile(t, n)
	gate := make(chan struct{})
	g.gate.Store(&gate)

	a, _ := submit(n.e, slow)
	eventually(t, "A sending", func() bool { return targetState(t, n.e, a) == TargetSending })
	b, _ := submit(n.e, importText(t, n.e, "B"))

	began := time.Now()
	if err := n.e.SetRuntimeState(bg, RuntimeSuspended); err != nil {
		t.Fatal(err)
	}
	if d := time.Since(began); d > 5*time.Second {
		t.Fatalf("suspending waited %v for a transfer stuck on the host", d) // it must cancel the transfer, not wait it out
	}
	ob := waitSend(t, n.e, b)
	if ob.Outcome != OutcomeFailed || ob.Targets[0].Error.Code != CodeSuspended {
		t.Fatalf("B = %+v", ob)
	}
	oa := waitSend(t, n.e, a)
	if oa.Outcome != OutcomeFailed || oa.Targets[0].Error == nil {
		t.Fatalf("A = %+v", oa)
	}
	// Resuming does not resend what failed.
	if err := n.e.SetRuntimeState(bg, RuntimeActive); err != nil {
		t.Fatal(err)
	}
	eventually(t, "reconnected", func() bool {
		ps, _ := n.e.ListPeers(bg)
		return len(ps) == 1 && ps[0].Connection == Connected
	})
	time.Sleep(200 * time.Millisecond)
	if l.count() != 0 {
		t.Fatal("a failed send was delivered after the reconnect")
	}
}

func TestOperationHoldsContentUntilFinished(t *testing.T) {
	n, _ := senderNode(t, nil)
	g, slow := gatedFile(t, n)
	gate := make(chan struct{})
	g.gate.Store(&gate)
	a, _ := submit(n.e, slow)
	eventually(t, "A sending", func() bool { return targetState(t, n.e, a) == TargetSending })

	_ = n.e.ReleaseContent(bg, slow.LeaseID) // the caller lets go; the transfer still needs it
	if len(n.e.cs.contents) != 1 {
		t.Fatal("content freed under a running transfer")
	}
	close(gate)
	waitSend(t, n.e, a)
	eventually(t, "content freed", func() bool {
		n.e.cs.mu.Lock()
		defer n.e.cs.mu.Unlock()
		return len(n.e.cs.contents) == 0
	})
}

func TestSendNeedsRunning(t *testing.T) {
	n := newContentNode(t, nil)
	h := importText(t, n.e, "x")
	if _, err := submit(n.e, h); code(err) != CodeInvalidState {
		t.Fatalf("before Start: %v", err)
	}
	start(t, n)
	_ = n.e.SetRuntimeState(bg, RuntimeSuspended)
	if _, err := submit(n.e, h); code(err) != CodeSuspended {
		t.Fatalf("suspended: %v", err)
	}
}

func TestOutcomeMatrix(t *testing.T) {
	st := func(s ...TargetState) []TargetStatus {
		var out []TargetStatus
		for _, x := range s {
			out = append(out, TargetStatus{State: x})
		}
		return out
	}
	for _, c := range []struct {
		in      []TargetStatus
		phase   Phase
		outcome Outcome
	}{
		{st(TargetQueued, TargetQueued), PhaseQueued, ""},
		{st(TargetQueued, TargetSending), PhaseSending, ""},
		{st(TargetSent, TargetQueued), PhaseSending, ""},
		{st(TargetSent, TargetSent), PhaseCompleted, OutcomeSuccess},
		{st(TargetSuperseded, TargetSuperseded), PhaseCompleted, OutcomeSuperseded},
		{st(TargetSent, TargetFailed), PhaseCompleted, OutcomePartial},
		{st(TargetSent, TargetSuperseded), PhaseCompleted, OutcomePartial},
		{st(TargetFailed, TargetFailed), PhaseCompleted, OutcomeFailed},
		{st(TargetFailed, TargetSuperseded), PhaseCompleted, OutcomeFailed},
	} {
		if p, o := phaseOf(c.in), outcomeOf(c.in); p != c.phase || o != c.outcome {
			t.Errorf("%v: %s/%s, want %s/%s", c.in, p, o, c.phase, c.outcome)
		}
	}
}

// contentOf reads a received clip: its representations by MIME type and its
// files by name.
func contentOf(t *testing.T, e *Engine, r ReceivedContent) (reps, files map[string]string) {
	t.Helper()
	l, err := e.RetainContent(bg, r.Content.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer e.ReleaseContent(bg, l.LeaseID)
	reps, files = map[string]string{}, map[string]string{}
	for _, p := range l.Content.Parts {
		if p.Kind == PartFile {
			files[p.Name] = readAll(t, e, l.LeaseID, p.ID)
		} else {
			reps[p.MIME] = readAll(t, e, l.LeaseID, p.ID)
		}
	}
	return
}
