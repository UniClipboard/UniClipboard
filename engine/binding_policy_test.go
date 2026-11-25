package engine

import (
	"bytes"
	"image"
	"image/color"
	"image/png"
	"io"
	"strings"
	"testing"
)

func observeAutoContent(t *testing.T, e *Engine, epoch string, seq uint64, in ContentInput) ObservationResult {
	t.Helper()
	h, err := e.ImportContent(bg, in)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.ReleaseContent(bg, h.LeaseID) }()
	res, err := e.ObserveClipboard(bg, Observation{Epoch: epoch, Sequence: seq, Mode: ObserveChange, LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	return res
}

func startAutoBaseline(t *testing.T, e *Engine, epoch string) {
	t.Helper()
	if _, err := e.ObserveClipboard(bg, Observation{Epoch: epoch, Sequence: 1, Mode: ObserveBaseline}); err != nil {
		t.Fatal(err)
	}
}

func readReceiptPart(t *testing.T, e *Engine, rc ReceivedContent, idx int) string {
	t.Helper()
	h, err := e.RetainContent(bg, rc.Content.ID)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = e.ReleaseContent(bg, h.LeaseID) }()
	r, err := e.ReadContent(bg, h.LeaseID, rc.Content.Parts[idx].ID)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = r.Close() }()
	b, err := io.ReadAll(r)
	if err != nil {
		t.Fatal(err)
	}
	return string(b)
}

func tinyPNG(t *testing.T, r, g, b uint8) []byte {
	t.Helper()
	img := image.NewRGBA(image.Rect(0, 0, 2, 2))
	img.Set(0, 0, color.RGBA{R: r, G: g, B: b, A: 255})
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		t.Fatal(err)
	}
	return buf.Bytes()
}

// TestBindingPolicyDirectionsAndExplicitBypass verifies:
// 1. Plan A per-type direction policy (bidirectional / send_only / receive_only / disabled).
// 2. Explicit manual sends ignore automatic binding policy (Decision #2).
// 3. Updating binding policy preserves live sessions and affects only subsequent new transfers (Decision #3).
// 4. Upper-layer backend policy ceiling cannot be bypassed by binding policy.
func TestBindingPolicyDirectionsAndExplicitBypass(t *testing.T) {
	a, b := pairedPair(t, nil)
	startAutoBaseline(t, a.e, "ep-a")
	startAutoBaseline(t, b.e, "ep-b")

	devB, err := a.e.GetDevice(bg, string(selfID(b.e)))
	if err != nil || len(devB.Bindings) != 1 {
		t.Fatalf("devB = %+v, err=%v", devB, err)
	}
	bindAB := devB.Bindings[0]
	if bindAB.Policy != DefaultBindingSyncPolicy() {
		t.Fatalf("default binding policy = %+v", bindAB.Policy)
	}

	// Set A's binding to B: Text=receive_only, Images=send_only, Files=disabled.
	polAB := BindingSyncPolicy{
		Text:   SyncDirectionReceiveOnly,
		Images: SyncDirectionSendOnly,
		Files:  SyncDirectionDisabled,
	}
	updated, err := a.e.SetBindingPolicy(bg, SetBindingPolicyRequest{
		ID:               bindAB.ID,
		ExpectedRevision: bindAB.Revision,
		Policy:           polAB,
	})
	if err != nil {
		t.Fatal(err)
	}
	if updated.Policy != polAB || updated.Revision != bindAB.Revision+1 {
		t.Fatalf("updated binding = %+v", updated)
	}
	// Decision #3: updating policy does NOT disconnect the existing live session.
	if updated.Connection != Connected {
		t.Fatalf("live session dropped after SetBindingPolicy: connection=%q", updated.Connection)
	}

	// CAS conflict and invalid policy rejection.
	if _, err := a.e.SetBindingPolicy(bg, SetBindingPolicyRequest{ID: bindAB.ID, ExpectedRevision: bindAB.Revision, Policy: polAB}); code(err) != CodeConflict {
		t.Fatalf("stale revision err = %v, want conflict", err)
	}
	if _, err := a.e.SetBindingPolicy(bg, SetBindingPolicyRequest{ID: bindAB.ID, ExpectedRevision: updated.Revision, Policy: BindingSyncPolicy{Text: "bad", Images: SyncDirectionBidirectional, Files: SyncDirectionBidirectional}}); code(err) != CodeInvalidArgument {
		t.Fatalf("invalid policy err = %v, want invalid_argument", err)
	}

	// 1. Automatic text copy on A is blocked by Text=receive_only (filtered, not sent to B).
	resText := observeAutoContent(t, a.e, "ep-a", 2, ContentInput{
		Representations: []RepresentationInput{{MIME: MimeText, Size: 9, Reader: strings.NewReader("auto-text")}},
	})
	if resText.Disposition != DispDisabled || resText.OperationID != "" {
		t.Fatalf("auto text result = %+v, want disabled with no operation", resText)
	}
	if got := len(received(b.e)); got != 0 {
		t.Fatalf("B received %d items after auto text filtered, want 0", got)
	}

	// 2. Decision #2: Explicit manual send on A ignores automatic binding policy and delivers text to B!
	if o := sendText(t, a, b, "explicit-text"); o.Outcome != OutcomeSuccess {
		t.Fatalf("explicit send with Text=receive_only failed: %+v", o)
	}
	waitReceived(t, b.e, 1)
	if got := readReceiptPart(t, b.e, received(b.e)[0], 0); got != "explicit-text" {
		t.Fatalf("B received %q, want explicit-text", got)
	}

	// 3. Automatic image copy on A IS allowed by Images=send_only.
	pngBytes := tinyPNG(t, 10, 20, 30)
	resImg := observeAutoContent(t, a.e, "ep-a", 3, ContentInput{
		Representations: []RepresentationInput{{MIME: MimePNG, Size: int64(len(pngBytes)), Reader: bytes.NewReader(pngBytes)}},
	})
	if resImg.Disposition != DispSubmitted || resImg.OperationID == "" {
		t.Fatalf("auto image disposition = %+v, want submitted", resImg)
	}
	if o := waitSend(t, a.e, resImg.OperationID); o.Outcome != OutcomeSuccess {
		t.Fatalf("auto image send outcome = %+v", o)
	}
	waitReceived(t, b.e, 2)

	// 4. Receive path on A:
	//    - Image from B is BLOCKED (Images=send_only).
	//    - Text from B is ALLOWED (Text=receive_only).
	resBImg := observeAutoContent(t, b.e, "ep-b", 2, ContentInput{
		Representations: []RepresentationInput{{MIME: MimePNG, Size: int64(len(pngBytes)), Reader: bytes.NewReader(pngBytes)}},
	})
	if resBImg.Disposition != DispSubmitted {
		t.Fatalf("B auto image = %+v", resBImg)
	}
	_ = waitSend(t, b.e, resBImg.OperationID)

	resBText := observeAutoContent(t, b.e, "ep-b", 3, ContentInput{
		Representations: []RepresentationInput{{MIME: MimeText, Size: 11, Reader: strings.NewReader("from-b-text")}},
	})
	if resBText.Disposition != DispSubmitted {
		t.Fatalf("B auto text = %+v", resBText)
	}
	if o := waitSend(t, b.e, resBText.OperationID); o.Outcome != OutcomeSuccess {
		t.Fatalf("B auto text send = %+v", o)
	}
	waitReceived(t, a.e, 1)
	if rs := received(a.e); len(rs) != 1 || readReceiptPart(t, a.e, rs[0], 0) != "from-b-text" {
		t.Fatalf("A received = %+v, want only from-b-text (PNG must have been discarded)", rs)
	}

	// 5. Mixed [Text + PNG] from B to A: A keeps Text and strips PNG per receive policy!
	resBMixed := observeAutoContent(t, b.e, "ep-b", 4, ContentInput{
		Representations: []RepresentationInput{
			{MIME: MimeText, Size: 10, Reader: strings.NewReader("mixed-text")},
			{MIME: MimePNG, Size: int64(len(pngBytes)), Reader: bytes.NewReader(pngBytes)},
		},
	})
	if resBMixed.Disposition != DispSubmitted {
		t.Fatalf("B mixed = %+v", resBMixed)
	}
	if o := waitSend(t, b.e, resBMixed.OperationID); o.Outcome != OutcomeSuccess {
		t.Fatalf("B mixed send = %+v", o)
	}
	waitReceived(t, a.e, 2)
	latest := received(a.e)[0]
	if len(latest.Content.Parts) != 1 || latest.Content.Parts[0].MIME != MimeText {
		t.Fatalf("mixed receive parts = %+v, want only text/plain", latest.Content.Parts)
	}

	// 6. Upper-layer ceiling: disabling Images at backend level cannot be bypassed by binding bidirectional.
	updateSettings(t, a.e, policy(BackendTailcat, func(p *BackendSyncPolicy) { p.Images = false }))
	if _, err := a.e.SetBindingPolicy(bg, SetBindingPolicyRequest{
		ID:               bindAB.ID,
		ExpectedRevision: updated.Revision,
		Policy:           DefaultBindingSyncPolicy(),
	}); err != nil {
		t.Fatal(err)
	}
	pngBytes2 := tinyPNG(t, 90, 80, 70)
	resCeil := observeAutoContent(t, a.e, "ep-a", 4, ContentInput{
		Representations: []RepresentationInput{{MIME: MimePNG, Size: int64(len(pngBytes2)), Reader: bytes.NewReader(pngBytes2)}},
	})
	if resCeil.Disposition != DispDisabled || resCeil.OperationID != "" {
		t.Fatalf("backend ceiling bypassed: res=%+v", resCeil)
	}

	// 7. Subsequent new transfer after policy update takes effect immediately on the same connection.
	resNext := observeAutoContent(t, a.e, "ep-a", 5, ContentInput{
		Representations: []RepresentationInput{{MIME: MimeText, Size: 10, Reader: strings.NewReader("after-edit")}},
	})
	if resNext.Disposition != DispSubmitted {
		t.Fatalf("text after reset to bidirectional = %+v", resNext)
	}
	if o := waitSend(t, a.e, resNext.OperationID); o.Outcome != OutcomeSuccess {
		t.Fatalf("send after reset = %+v", o)
	}
	waitReceived(t, b.e, 3)
	if got := readReceiptPart(t, b.e, received(b.e)[0], 0); got != "after-edit" {
		t.Fatalf("B latest = %q, want after-edit", got)
	}
}
