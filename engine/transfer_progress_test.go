package engine

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/json"
	"io"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	peersess "github.com/uniclipboard/unic/internal/session"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// sessionPlugin wraps *peersess.Backend as an in-process peer_sessions plugin
// so tests can configure AckTimeout, DisableClipAck, or hold inbound Receive.
type sessionPlugin struct {
	b           *peersess.Backend
	holdReceive chan struct{}
	paceDelay   time.Duration
}

func newSessionPlugin(cfg peersess.Config) *sessionPlugin {
	return &sessionPlugin{b: peersess.New(cfg)}
}

type holdingReceiver struct {
	plugin.PeerReceiver
	hold chan struct{}
}

func (h holdingReceiver) Receive(ctx context.Context, receiveID string, parts plugin.ReceiveParts) (string, error) {
	select {
	case <-h.hold:
	case <-ctx.Done():
		return "", ctx.Err()
	}
	return h.PeerReceiver.Receive(ctx, receiveID, parts)
}

func (p *sessionPlugin) Spec() plugin.Spec {
	return plugin.Spec{ID: tailcat.ID, Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend}, TransportKind: plugin.TransportPeerSessions}
}
func (p *sessionPlugin) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (p *sessionPlugin) Start(context.Context) error                            { return nil }
func (p *sessionPlugin) Close(ctx context.Context) error                        { return p.b.Close(ctx) }
func (p *sessionPlugin) SetPeerAuthority(a plugin.PeerAuthority)                { p.b.SetPeerAuthority(a) }
func (p *sessionPlugin) SetPeerReceiver(r plugin.PeerReceiver) {
	if p.holdReceive != nil {
		r = holdingReceiver{PeerReceiver: r, hold: p.holdReceive}
	}
	if p.paceDelay > 0 {
		r = pacedReceiver{inner: r, delay: p.paceDelay}
	}
	p.b.SetPeerReceiver(r)
}
func (p *sessionPlugin) Bind(ctx context.Context, b rpc.Bind) error             { return p.b.Bind(ctx, b) }
func (p *sessionPlugin) ApplyState(ctx context.Context, s rpc.SyncState) error {
	return p.b.ApplyState(ctx, s)
}
func (p *sessionPlugin) PairingAddress(ctx context.Context, purpose string) (string, error) {
	return p.b.PairingAddress(ctx, purpose)
}
func (p *sessionPlugin) Connect(ctx context.Context, c rpc.Connect) (rpc.ConnectResult, error) {
	return p.b.Connect(ctx, c)
}
func (p *sessionPlugin) Send(ctx context.Context, s rpc.Send, src plugin.SendSource) (rpc.SendResult, error) {
	return p.b.Send(ctx, s, src)
}

func pairedCustomNodes(t *testing.T, aCfg, bCfg peersess.Config) (node, node) {
	t.Helper()
	if aCfg.Transport == nil {
		aCfg.Transport = newFakeNet(t)
	}
	if bCfg.Transport == nil {
		bCfg.Transport = newFakeNet(t)
	}
	a := newNodeWithPlugin(t, newSessionPlugin(aCfg))
	b := newNodeWithPlugin(t, newSessionPlugin(bCfg))
	pairCustomPair(t, a, b)
	return a, b
}

func newNodeWithPlugin(t *testing.T, p plugin.Plugin) node {
	t.Helper()
	dir := t.TempDir()
	e, err := New(bg, Options{
		DataDir:  dir,
		Settings: DefaultSettings(),
		Plugins:  []PluginInstance{{Plugin: p, Builtin: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := e.Start(bg); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = e.Stop(bg)
		_ = e.Close(bg)
	})
	return node{e: e, dir: dir}
}

func approvePendingDevice(t *testing.T, e *Engine, peerID DeviceID) {
	t.Helper()
	var dev Device
	eventually(t, "pending device", func() bool {
		d, err := e.GetDevice(bg, string(peerID))
		if err == nil && len(d.Bindings) > 0 {
			dev = d
			return true
		}
		return false
	})
	bindings := make([]BindingRequest, 0, len(dev.Bindings))
	for _, b := range dev.Bindings {
		if b.State == BindingPending {
			bindings = append(bindings, BindingRequest{ID: b.ID, ExpectedRevision: b.Revision})
		}
	}
	if _, err := e.ApproveDevice(bg, ApproveDeviceRequest{
		ID:                  dev.ID,
		ExpectedRevision:    dev.Revision,
		ExpectedFingerprint: dev.Fingerprint,
		Bindings:            bindings,
	}); err != nil {
		t.Fatal(err)
	}
}

func waitPeerConnected(t *testing.T, e *Engine, peerID DeviceID) {
	t.Helper()
	eventually(t, "peer connected", func() bool {
		d, err := e.GetDevice(bg, string(peerID))
		if err != nil {
			return false
		}
		for _, b := range d.Bindings {
			if b.Connection == Connected {
				return true
			}
		}
		return false
	})
}

func pairCustomPair(t *testing.T, a, b node) {
	t.Helper()
	tk, err := b.e.ExportPairingTicket(bg)
	if err != nil {
		t.Fatal(err)
	}
	op, err := a.e.JoinByTicket(bg, JoinTicketRequest{RequestID: RequestID(randHex(8)), Ticket: tk.Ticket})
	if err != nil {
		t.Fatal(err)
	}
	waitOp(t, a.e, op, PhaseSucceeded)
	approvePendingDevice(t, b.e, selfID(a.e))
	approvePendingDevice(t, a.e, selfID(b.e))
	waitPeerConnected(t, a.e, selfID(b.e))
	waitPeerConnected(t, b.e, selfID(a.e))
}

func TestTransferProgressAndRemoteReceiptEndToEnd(t *testing.T) {
	a, b := pairedPair(t, nil)

	payload := bytes.Repeat([]byte("0123456789abcdef"), 128<<10) // 2 MiB
	wantHash := sha256.Sum256(payload)
	secretFile := "secret-project-plan-2026.pdf"

	sub, err := a.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()

	h, err := a.e.ImportContent(bg, ContentInput{
		Files: []FileInput{{
			Name:   secretFile,
			MIME:   "application/pdf",
			Size:   int64(len(payload)),
			Reader: bytes.NewReader(payload),
		}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a.e.ReleaseContent(bg, h.LeaseID) }()

	res, err := a.e.CommitContent(bg, CommitRequest{RequestID: "prog-e2e-1", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	if res.HistoryID == "" || res.OperationID == "" {
		t.Fatalf("commit result = %+v, want history and operation IDs", res)
	}

	op := waitSend(t, a.e, res.OperationID)
	if op.HistoryID != res.HistoryID {
		t.Fatalf("op.HistoryID = %q, want %q", op.HistoryID, res.HistoryID)
	}
	if op.Outcome != OutcomeSuccess || len(op.Targets) != 1 {
		t.Fatalf("op = %+v, want success with 1 target", op)
	}
	tgt := op.Targets[0]
	if tgt.State != TargetSent || tgt.ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("target = %+v, want sent + remote_received", tgt)
	}
	if tgt.Progress == nil || tgt.Progress.BytesSent != int64(len(payload)) || tgt.Progress.TotalBytes != int64(len(payload)) {
		t.Fatalf("progress = %+v, want %d/%d", tgt.Progress, len(payload), len(payload))
	}

	// Verify ListOperations includes this operation and contains no filenames.
	ops, err := a.e.ListOperations(bg)
	if err != nil {
		t.Fatal(err)
	}
	if len(ops) == 0 || ops[0].ID != op.ID {
		t.Fatalf("ListOperations[0] = %+v, want %s", ops, op.ID)
	}
	raw, _ := json.Marshal(ops)
	if strings.Contains(string(raw), secretFile) {
		t.Fatalf("operation JSON leaked filename: %s", raw)
	}

	// Because ReceiptStageRemoteReceived was returned, receiver must ALREADY have the committed item.
	rs := received(b.e)
	if len(rs) != 1 {
		t.Fatalf("receiver has %d items immediately after remote_received, want 1", len(rs))
	}
	gotBytes := []byte(readReceiptPart(t, b.e, rs[0], 0))
	if sha256.Sum256(gotBytes) != wantHash {
		t.Fatalf("received payload hash mismatch")
	}
}

func TestBidirectionalConcurrentLargeFileTransferNoDeadlock(t *testing.T) {
	a, b := pairedPair(t, nil)

	bufA := bytes.Repeat([]byte("A1B2C3D4"), 256<<10) // 2 MiB
	bufB := bytes.Repeat([]byte("Z9Y8X7W6"), 256<<10) // 2 MiB
	hashA := sha256.Sum256(bufA)
	hashB := sha256.Sum256(bufB)

	ha, err := a.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "a.bin", Size: int64(len(bufA)), Reader: bytes.NewReader(bufA)}}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a.e.ReleaseContent(bg, ha.LeaseID) }()
	hb, err := b.e.ImportContent(bg, ContentInput{Files: []FileInput{{Name: "b.bin", Size: int64(len(bufB)), Reader: bytes.NewReader(bufB)}}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = b.e.ReleaseContent(bg, hb.LeaseID) }()

	var wg sync.WaitGroup
	var opA, opB Operation
	var errA, errB error
	wg.Add(2)
	go func() {
		defer wg.Done()
		r, err := a.e.CommitContent(bg, CommitRequest{RequestID: "bidir-a", LeaseID: ha.LeaseID})
		if err != nil {
			errA = err
			return
		}
		opA = waitSend(t, a.e, r.OperationID)
	}()
	go func() {
		defer wg.Done()
		r, err := b.e.CommitContent(bg, CommitRequest{RequestID: "bidir-b", LeaseID: hb.LeaseID})
		if err != nil {
			errB = err
			return
		}
		opB = waitSend(t, b.e, r.OperationID)
	}()
	wg.Wait()

	if errA != nil || errB != nil {
		t.Fatalf("commit errors: a=%v b=%v", errA, errB)
	}
	if opA.Outcome != OutcomeSuccess || opA.Targets[0].ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("opA = %+v, want success/remote_received", opA)
	}
	if opB.Outcome != OutcomeSuccess || opB.Targets[0].ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("opB = %+v, want success/remote_received", opB)
	}
	if got := sha256.Sum256([]byte(readReceiptPart(t, b.e, received(b.e)[0], 0))); got != hashA {
		t.Fatal("B received wrong bytes from A")
	}
	if got := sha256.Sum256([]byte(readReceiptPart(t, a.e, received(a.e)[0], 0))); got != hashB {
		t.Fatal("A received wrong bytes from B")
	}
}

func TestRemoteDiscardedMapsToTargetDiscarded(t *testing.T) {
	a, b := pairedPair(t, nil)

	// Turn off file sync on B so B cleanly drains and discards incoming files.
	updateSettings(t, b.e, func(s *Settings) {
		s.Sync.Files = false
	})

	fileData := bytes.Repeat([]byte("file-bytes"), 1024)
	h, err := a.e.ImportContent(bg, ContentInput{
		Files: []FileInput{{Name: "blocked.bin", Size: int64(len(fileData)), Reader: bytes.NewReader(fileData)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a.e.ReleaseContent(bg, h.LeaseID) }()

	res, err := a.e.CommitContent(bg, CommitRequest{RequestID: "discard-1", LeaseID: h.LeaseID})
	if err != nil {
		t.Fatal(err)
	}
	op := waitSend(t, a.e, res.OperationID)
	if op.Outcome != OutcomeFailed || len(op.Targets) != 1 {
		t.Fatalf("op = %+v, want failed (discarded must not be OutcomeSuccess)", op)
	}
	if op.Targets[0].State != TargetDiscarded || op.Targets[0].ReceiptStage != ReceiptStageRemoteDiscarded {
		t.Fatalf("target = %+v, want discarded/remote_discarded", op.Targets[0])
	}

	// Session must remain alive and deliver subsequent allowed text cleanly.
	op2 := sendText(t, a, b, "after-discard")
	if op2.Outcome != OutcomeSuccess || op2.Targets[0].ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("subsequent send = %+v, want success/remote_received", op2)
	}
	waitReceived(t, b.e, 1)
}

func TestLegacyPeerWithoutClipAckFallsBackToLocalWritten(t *testing.T) {
	a, b := pairedCustomNodes(t, peersess.Config{}, peersess.Config{DisableClipAck: true})
	op := sendText(t, a, b, "hello-legacy")
	if op.Outcome != OutcomeSuccess || len(op.Targets) != 1 {
		t.Fatalf("op = %+v, want success", op)
	}
	if op.Targets[0].State != TargetSent || op.Targets[0].ReceiptStage != ReceiptStageLocalWritten {
		t.Fatalf("target = %+v, want sent/local_written for legacy peer", op.Targets[0])
	}
	waitReceived(t, b.e, 1)
}

func TestAckTimeoutPreservesLocalWrittenUncertainOutcome(t *testing.T) {
	// Hold receiver's Receive so clip_ack cannot arrive before AckTimeout.
	hold := make(chan struct{})
	defer close(hold)

	a := newNodeWithPlugin(t, newSessionPlugin(peersess.Config{Transport: newFakeNet(t), AckTimeout: 250 * time.Millisecond}))
	bp := newSessionPlugin(peersess.Config{Transport: newFakeNet(t)})
	bp.holdReceive = hold
	b := newNodeWithPlugin(t, bp)
	pairCustomPair(t, a, b)

	id, err := submit(a.e, importText(t, a.e, "ack-timeout-body"))
	if err != nil {
		t.Fatal(err)
	}
	op := waitSend(t, a.e, id)
	if op.Outcome != OutcomeFailed || len(op.Targets) != 1 {
		t.Fatalf("op = %+v, want failed on ACK timeout", op)
	}
	tgt := op.Targets[0]
	if tgt.State != TargetFailed || tgt.Error == nil || tgt.Error.Code != CodeTimeout {
		t.Fatalf("target = %+v, want failed/timeout", tgt)
	}
	if tgt.ReceiptStage != ReceiptStageLocalWritten {
		t.Fatalf("target.ReceiptStage = %q, want local_written (uncertain remote outcome)", tgt.ReceiptStage)
	}
	if tgt.Progress == nil || tgt.Progress.BytesSent != tgt.Progress.TotalBytes {
		t.Fatalf("target.Progress = %+v, want full local bytes written", tgt.Progress)
	}
}

func TestRevocationCancelsAckWaitImmediately(t *testing.T) {
	hold := make(chan struct{})
	defer close(hold)

	a := newNodeWithPlugin(t, newSessionPlugin(peersess.Config{Transport: newFakeNet(t), AckTimeout: 20 * time.Second}))
	bp := newSessionPlugin(peersess.Config{Transport: newFakeNet(t)})
	bp.holdReceive = hold
	b := newNodeWithPlugin(t, bp)
	pairCustomPair(t, a, b)

	id, err := submit(a.e, importText(t, a.e, "revoke-during-ack"))
	if err != nil {
		t.Fatal(err)
	}

	// Wait until target reaches awaiting_receipt (local_written while still sending).
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		op, err := a.e.GetOperation(bg, id)
		if err == nil && len(op.Targets) == 1 && op.Targets[0].State == TargetSending && op.Targets[0].ReceiptStage == ReceiptStageLocalWritten {
			break
		}
		time.Sleep(15 * time.Millisecond)
	}

	start := time.Now()
	dev, err := a.e.GetDevice(bg, string(selfID(b.e)))
	if err != nil {
		t.Fatal(err)
	}
	if err := a.e.UnpairDevice(bg, UnpairDeviceRequest{ID: dev.ID, ExpectedRevision: dev.Revision}); err != nil {
		t.Fatal(err)
	}
	if elapsed := time.Since(start); elapsed > 5*time.Second {
		t.Fatalf("UnpairDevice took %v while waiting for clip_ack, want immediate cancellation", elapsed)
	}
	op := waitSend(t, a.e, id)
	if op.Outcome != OutcomeFailed {
		t.Fatalf("op = %+v, want failed after unpair", op)
	}
}

func TestProgressEventsAreThrottledAndOutcomeMatrixHandlesDiscarded(t *testing.T) {
	// Verify Outcome matrix for TargetDiscarded.
	st := func(ss ...TargetState) []TargetStatus {
		out := make([]TargetStatus, len(ss))
		for i, s := range ss {
			out[i] = TargetStatus{State: s}
		}
		return out
	}
	if got := outcomeOf(st(TargetDiscarded)); got != OutcomeFailed {
		t.Fatalf("outcomeOf(discarded) = %q, want failed", got)
	}
	if got := outcomeOf(st(TargetSent, TargetDiscarded)); got != OutcomePartial {
		t.Fatalf("outcomeOf(sent, discarded) = %q, want partial", got)
	}
	if got := outcomeOf(st(TargetDiscarded, TargetSuperseded)); got != OutcomeFailed {
		t.Fatalf("outcomeOf(discarded, superseded) = %q, want failed", got)
	}

	// Verify progress event throttling prevents EventBuffer(128) overflow.
	n := newContentNode(t, nil)
	sub, err := n.e.SubscribeEvents(bg)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = sub.Close() }()

	k := targetKey{backend: BackendTailcat, kind: TargetDevice, id: "peer-1"}
	n.e.mu.Lock()
	o, _, gerr := n.e.newOpLocked(OperationSend, "req-throttle", [32]byte{1}, PhaseSending)
	if gerr != nil {
		n.e.mu.Unlock()
		t.Fatal(gerr)
	}
	o.op.Targets = []TargetStatus{{BackendID: k.backend, TargetKind: k.kind, TargetID: k.id, PeerID: DeviceID(k.id), State: TargetSending}}
	opID := o.op.ID
	n.e.mu.Unlock()

	for i := int64(1); i <= 500; i++ {
		n.e.reportTargetProgress(opID, k, "", TransferProgress{BytesSent: i * 1024, TotalBytes: 500 * 1024, PartCount: 1})
	}
	// Transition to awaiting_receipt must emit immediately even within the throttle window.
	n.e.reportTargetProgress(opID, k, ReceiptStageLocalWritten, TransferProgress{BytesSent: 500 * 1024, TotalBytes: 500 * 1024, PartCount: 1})

	// Drain available events without resync_required overflow.
	var opEvents int
	drainCtx, cancel := context.WithTimeout(bg, 100*time.Millisecond)
	defer cancel()
	for {
		ev, err := sub.Next(drainCtx)
		if err != nil {
			if code(err) == CodeResyncRequired {
				t.Fatalf("progress updates overflowed event buffer: %v", err)
			}
			break
		}
		if ev.Kind == EventOperations {
			opEvents++
		}
	}
	if opEvents > 10 {
		t.Fatalf("expected coalesced EventOperations count <= 10, got %d", opEvents)
	}
}

type blockingReceivedPlugin struct {
	hold chan struct{}
}

func (p *blockingReceivedPlugin) Spec() plugin.Spec {
	return plugin.Spec{ID: "test.blocker", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}
}
func (p *blockingReceivedPlugin) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (p *blockingReceivedPlugin) Start(context.Context) error                            { return nil }
func (p *blockingReceivedPlugin) Close(context.Context) error                            { return nil }
func (p *blockingReceivedPlugin) Process(ctx context.Context, _ plugin.ProcessContext) error {
	select {
	case <-p.hold:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

// Keep unused import check satisfied if helper evolves.
var _ io.Reader

type pacedParts struct {
	inner plugin.ReceiveParts
	delay time.Duration
}

func (p pacedParts) Next(ctx context.Context) (int, io.ReadCloser, error) {
	idx, body, err := p.inner.Next(ctx)
	if err != nil {
		return idx, nil, err
	}
	return idx, &pacedBody{ReadCloser: body, delay: p.delay}, nil
}

type pacedBody struct {
	io.ReadCloser
	delay time.Duration
}

func (b *pacedBody) Read(p []byte) (int, error) {
	if len(p) > 32*1024 {
		p = p[:32*1024]
	}
	if b.delay > 0 {
		time.Sleep(b.delay)
	}
	return b.ReadCloser.Read(p)
}

type pacedReceiver struct {
	inner plugin.PeerReceiver
	delay time.Duration
}

func (r pacedReceiver) OpenReceive(ctx context.Context, o rpc.ReceiveOpen) (rpc.ReceiveGrant, error) {
	return r.inner.OpenReceive(ctx, o)
}

func (r pacedReceiver) Receive(ctx context.Context, id string, parts plugin.ReceiveParts) (string, error) {
	return r.inner.Receive(ctx, id, pacedParts{inner: parts, delay: r.delay})
}

func TestReceiveProgressAndMidTransferUserDiscardPreservesSession(t *testing.T) {
	// Pace receiver reads slightly so the test deterministically observes
	// mid-flight receive progress and triggers DiscardReceive before completion.
	a := newNodeWithPlugin(t, newSessionPlugin(peersess.Config{Transport: newFakeNet(t)}))
	bp := newSessionPlugin(peersess.Config{Transport: newFakeNet(t)})
	bp.paceDelay = 15 * time.Millisecond
	b := newNodeWithPlugin(t, bp)
	pairCustomPair(t, a, b)

	// 1. Transfer a 1 MiB file and observe monotonic receive progress + commit.
	payload1 := bytes.Repeat([]byte("rx-prog-"), 128*1024) // 1 MiB
	h1, err := a.e.ImportContent(bg, ContentInput{
		Files: []FileInput{{Name: "progress-ok.bin", Size: int64(len(payload1)), Reader: bytes.NewReader(payload1)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a.e.ReleaseContent(bg, h1.LeaseID) }()

	res1, err := a.e.CommitContent(bg, CommitRequest{RequestID: "rx-prog-1", LeaseID: h1.LeaseID})
	if err != nil {
		t.Fatal(err)
	}

	var sawIntermediate bool
	var rxOp1ID OperationID
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		ops, err := b.e.ListOperations(bg)
		if err == nil {
			for _, op := range ops {
				if op.Kind != OperationReceive || op.Receive == nil {
					continue
				}
				rxOp1ID = op.ID
				p := op.Receive.Progress
				if op.Receive.State == ReceiveReceiving && p.BytesReceived > 0 && p.BytesReceived < p.TotalBytes {
					sawIntermediate = true
				}
				if op.Receive.State == ReceiveCommitted {
					goto rx1Done
				}
			}
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatal("timed out waiting for receive op 1 commit")
rx1Done:
	if !sawIntermediate {
		t.Fatal("never observed intermediate receive bytes progress on receiver")
	}
	opSend1 := waitSend(t, a.e, res1.OperationID)
	if opSend1.Outcome != OutcomeSuccess || opSend1.Targets[0].ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("send1 = %+v, want success/remote_received", opSend1)
	}
	// Discarding an already-committed receive returns conflict.
	if err := b.e.DiscardReceive(bg, rxOp1ID); code(err) != CodeConflict {
		t.Fatalf("DiscardReceive(committed) = %v, want conflict", err)
	}
	// Discarding a non-receive op returns invalid_argument; unknown returns not_found.
	if err := a.e.DiscardReceive(bg, res1.OperationID); code(err) != CodeInvalidArgument {
		t.Fatalf("DiscardReceive(send) = %v, want invalid_argument", err)
	}
	if err := b.e.DiscardReceive(bg, "nonexistent-op"); code(err) != CodeResourceExpired {
		t.Fatalf("DiscardReceive(foreign) = %v, want resource_expired", err)
	}
	if err := b.e.DiscardReceive(bg, OperationID(string(b.e.instance)+"-999999")); code(err) != CodeNotFound {
		t.Fatalf("DiscardReceive(unknown) = %v, want not_found", err)
	}

	// 2. Transfer a second 2 MiB file and call DiscardReceive mid-flight!
	payload2 := bytes.Repeat([]byte("rx-drop-"), 256*1024) // 2 MiB
	h2, err := a.e.ImportContent(bg, ContentInput{
		Files: []FileInput{{Name: "discard-me.bin", Size: int64(len(payload2)), Reader: bytes.NewReader(payload2)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = a.e.ReleaseContent(bg, h2.LeaseID) }()

	res2, err := a.e.CommitContent(bg, CommitRequest{RequestID: "rx-drop-2", LeaseID: h2.LeaseID})
	if err != nil {
		t.Fatal(err)
	}

	var rxOp2ID OperationID
	deadline = time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		ops, err := b.e.ListOperations(bg)
		if err == nil {
			for _, op := range ops {
				if op.Kind == OperationReceive && op.ID != rxOp1ID && op.Receive != nil && op.Receive.State == ReceiveReceiving && op.Receive.Progress.BytesReceived > 0 {
					rxOp2ID = op.ID
					break
				}
			}
		}
		if rxOp2ID != "" {
			break
		}
		time.Sleep(15 * time.Millisecond)
	}
	if rxOp2ID == "" {
		t.Fatal("did not observe second receive in flight before completion")
	}

	if err := b.e.DiscardReceive(bg, rxOp2ID); err != nil {
		t.Fatalf("DiscardReceive(in-flight) = %v", err)
	}
	// Idempotent second call must succeed.
	if err := b.e.DiscardReceive(bg, rxOp2ID); err != nil {
		t.Fatalf("DiscardReceive(idempotent) = %v", err)
	}

	// Receiver op must be immediately discarded; sender must receive clip_ack(discarded).
	rxOp2, err := b.e.GetOperation(bg, rxOp2ID)
	if err != nil || rxOp2.Receive == nil || rxOp2.Receive.State != ReceiveDiscarded || !rxOp2.Done() {
		t.Fatalf("rxOp2 = %+v err=%v, want completed/discarded", rxOp2, err)
	}
	opSend2 := waitSend(t, a.e, res2.OperationID)
	if opSend2.Outcome != OutcomeFailed || len(opSend2.Targets) != 1 || opSend2.Targets[0].State != TargetDiscarded || opSend2.Targets[0].ReceiptStage != ReceiptStageRemoteDiscarded {
		t.Fatalf("send2 after user discard = %+v, want failed/TargetDiscarded/remote_discarded", opSend2)
	}
	// Discarded file must NEVER enter B's received list (still only payload1).
	if got := len(received(b.e)); got != 1 {
		t.Fatalf("B received count = %d, want 1 (discarded transfer must not commit)", got)
	}

	// 3. Session must remain alive and deliver a subsequent text clip cleanly!
	opSend3 := sendText(t, a, b, "after-user-discard")
	if opSend3.Outcome != OutcomeSuccess || opSend3.Targets[0].ReceiptStage != ReceiptStageRemoteReceived {
		t.Fatalf("send3 after user discard = %+v, want success/remote_received", opSend3)
	}
	waitReceived(t, b.e, 2)
}
