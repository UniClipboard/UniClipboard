package engine_test

import (
	"bytes"
	"context"
	"crypto/sha256"
	"errors"
	"io"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	"github.com/uniclipboard/unic/internal/relayserver"
)

// toggleTransport wraps a loopback TCP transport with a switch to simulate P2P
// being completely blocked/offline (e.g. mobile cold start or NAT outage) while
// HTTPS relay remains reachable.
type toggleTransport struct {
	mu          sync.Mutex
	ln          net.Listener
	addr        string
	blocked     atomic.Bool
	conns       map[net.Conn]struct{}
	onWriteHook func(n int)
}

type hookConn struct {
	net.Conn
	t *toggleTransport
}

func (c *hookConn) Write(p []byte) (int, error) {
	c.t.mu.Lock()
	hook := c.t.onWriteHook
	c.t.mu.Unlock()
	if hook != nil {
		hook(len(p))
	}
	return c.Conn.Write(p)
}

func (t *toggleTransport) SetWriteHook(fn func(n int)) {
	t.mu.Lock()
	t.onWriteHook = fn
	t.mu.Unlock()
}

func newToggleTransport() *toggleTransport {
	return &toggleTransport{conns: map[net.Conn]struct{}{}}
}

func (t *toggleTransport) Start() error {
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		return err
	}
	t.mu.Lock()
	t.ln = ln
	t.addr = ln.Addr().String()
	t.mu.Unlock()
	return nil
}

func (t *toggleTransport) Close() error {
	t.mu.Lock()
	ln := t.ln
	t.ln = nil
	conns := make([]net.Conn, 0, len(t.conns))
	for c := range t.conns {
		conns = append(conns, c)
	}
	t.mu.Unlock()
	for _, c := range conns {
		_ = c.Close()
	}
	if ln != nil {
		return ln.Close()
	}
	return nil
}

func (t *toggleTransport) Listen(ctx context.Context) (net.Listener, error) {
	t.mu.Lock()
	ln := t.ln
	t.mu.Unlock()
	if ln == nil {
		return nil, errors.New("closed")
	}
	return &toggleListener{Listener: ln, parent: t}, nil
}

type toggleListener struct {
	net.Listener
	parent *toggleTransport
}

func (l *toggleListener) Accept() (net.Conn, error) {
	for {
		c, err := l.Listener.Accept()
		if err != nil {
			return nil, err
		}
		if l.parent.blocked.Load() {
			_ = c.Close()
			continue
		}
		l.parent.track(c)
		return &hookConn{Conn: c, t: l.parent}, nil
	}
}

func (t *toggleTransport) track(c net.Conn) {
	t.mu.Lock()
	t.conns[c] = struct{}{}
	t.mu.Unlock()
}

func (t *toggleTransport) Dial(ctx context.Context, addr string) (net.Conn, error) {
	if t.blocked.Load() {
		return nil, errors.New("p2p blocked")
	}
	var d net.Dialer
	c, err := d.DialContext(ctx, "tcp", addr)
	if err != nil {
		return nil, err
	}
	t.track(c)
	return &hookConn{Conn: c, t: t}, nil
}

func (t *toggleTransport) Addr() string {
	t.mu.Lock()
	defer t.mu.Unlock()
	return t.addr
}

func (t *toggleTransport) Binding(addr string) ([]byte, error) {
	return []byte("toggle-bind:" + addr), nil
}

func (t *toggleTransport) SetBlocked(on bool) {
	t.blocked.Store(on)
	if on {
		t.mu.Lock()
		for c := range t.conns {
			_ = c.Close()
		}
		clear(t.conns)
		t.mu.Unlock()
	}
}

func startRelayTestEngine(t *testing.T, name, relayURL string, relayEnabled bool, relayMaxBytes int64, tr *toggleTransport) *engine.Engine {
	t.Helper()
	settings := engine.DefaultSettings()
	settings.DeviceName = name
	settings.Sync.Relay = engine.RelaySyncSettings{
		Enabled:  relayEnabled,
		URL:      relayURL,
		MaxBytes: relayMaxBytes,
	}
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	eng, err := engine.New(context.Background(), engine.Options{
		DataDir:  dir,
		Settings: settings,
		Plugins: []engine.PluginInstance{
			{Plugin: tailcat.NewOn(tr), Builtin: true},
		},
	})
	if err != nil {
		t.Fatalf("engine.New(%s): %v", name, err)
	}
	if err := eng.Start(context.Background()); err != nil {
		_ = eng.Close(context.Background())
		t.Fatalf("engine.Start(%s): %v", name, err)
	}
	t.Cleanup(func() { _ = eng.Close(context.Background()) })
	return eng
}

func waitUntil(t *testing.T, timeout time.Duration, desc string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", desc)
}

func pairAndConnect(t *testing.T, a, b *engine.Engine) (aID, bID string) {
	t.Helper()
	ctx := context.Background()
	stA, _ := a.GetState(ctx)
	stB, _ := b.GetState(ctx)
	aID, bID = string(stA.Self.ID), string(stB.Self.ID)

	var ticket string
	waitUntil(t, 5*time.Second, "export ticket", func() bool {
		tk, err := b.ExportPairingTicket(ctx)
		if err != nil {
			return false
		}
		ticket = tk.Ticket
		return ticket != ""
	})

	if _, err := a.JoinByTicket(ctx, engine.JoinTicketRequest{RequestID: "join-ab", Ticket: ticket}); err != nil {
		t.Fatalf("JoinByTicket: %v", err)
	}

	// Approve pending device on both sides.
	approvePending := func(host *engine.Engine, targetID string) {
		waitUntil(t, 5*time.Second, "pending device "+targetID, func() bool {
			ds, err := host.ListDevices(ctx)
			if err != nil {
				return false
			}
			for _, d := range ds {
				if d.ID == targetID && len(d.Bindings) > 0 {
					if d.Trust == "trusted" && d.Bindings[0].State == "authorized" {
						return true
					}
					var bs []engine.BindingRequest
					for _, bd := range d.Bindings {
						if bd.State == "pending" {
							bs = append(bs, engine.BindingRequest{ID: bd.ID, ExpectedRevision: bd.Revision})
						}
					}
					if len(bs) == 0 {
						return false
					}
					_, err := host.ApproveDevice(ctx, engine.ApproveDeviceRequest{
						ID:                  d.ID,
						ExpectedRevision:    d.Revision,
						ExpectedFingerprint: d.Fingerprint,
						Bindings:            bs,
					})
					return err == nil
				}
			}
			return false
		})
	}
	approvePending(b, aID)
	approvePending(a, bID)

	waitUntil(t, 5*time.Second, "p2p connected", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection == engine.Connected {
				return true
			}
		}
		return false
	})
	return aID, bID
}

func TestHybridRelayFallbackSingleAndMultiChunkAndRevocation(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{
		MaxTransferBytes: 10 << 20,
	})
	ts := httptest.NewServer(srv)
	defer func() {
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	a := startRelayTestEngine(t, "Phone", ts.URL, true, 10<<20, trA)
	b := startRelayTestEngine(t, "Mac", ts.URL, true, 10<<20, trB)

	aID, bID := pairAndConnect(t, a, b)

	// Wait for relay_profile exchange over P2P so both devices report RelayRouteReady.
	waitUntil(t, 5*time.Second, "relay route ready on Phone", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	// 1. While P2P is ONLINE: sending text must use P2P and make ZERO relay uploads.
	p2pText := []byte("direct-over-p2p")
	leaseP2P, err := a.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(p2pText)), Reader: bytes.NewReader(p2pText)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	resP2P, err := a.CommitContent(ctx, engine.CommitRequest{RequestID: "req-p2p-1", LeaseID: leaseP2P.LeaseID})
	if err != nil || resP2P.SyncDisposition != engine.SyncQueued {
		t.Fatalf("p2p commit failed: res=%+v err=%v", resP2P, err)
	}
	waitUntil(t, 5*time.Second, "p2p op complete", func() bool {
		op, err := a.GetOperation(ctx, resP2P.OperationID)
		return err == nil && op.Done() && len(op.Targets) == 1 && op.Targets[0].State == engine.TargetSent && op.Targets[0].TransportRoute == engine.RouteP2P
	})
	if srv.ActiveTransferCount() != 0 {
		t.Fatalf("expected 0 relay transfers during P2P send")
	}

	// 2. Block P2P completely (simulate cold-started phone with P2P offline).
	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p disconnected on Phone", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	// 2a. 0-wait single-chunk (1-RTT) text transfer over E2EE HTTP relay.
	relayText := []byte("instant-via-e2ee-relay")
	leaseRelay1, err := a.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(relayText)), Reader: bytes.NewReader(relayText)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	resRelay1, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-relay-single",
		LeaseID:   leaseRelay1.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil || resRelay1.SyncDisposition != engine.SyncQueued {
		t.Fatalf("relay single commit failed: res=%+v err=%v", resRelay1, err)
	}
	waitUntil(t, 5*time.Second, "relay single op complete", func() bool {
		op, err := a.GetOperation(ctx, resRelay1.OperationID)
		if err != nil || !op.Done() || len(op.Targets) != 1 {
			return false
		}
		tg := op.Targets[0]
		return tg.State == engine.TargetSent && tg.TransportRoute == engine.RouteRelay && tg.ReceiptStage == engine.ReceiptStageRelayStored
	})

	// Verify Mac received the text via relay SSE and acked (server cleaned up).
	waitUntil(t, 5*time.Second, "Mac received relay text", func() bool {
		page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
		if err != nil {
			return false
		}
		for _, it := range page.Items {
			if it.Origin.Kind == engine.OriginKindRemoteDevice && it.Origin.DeviceID == engine.DeviceID(aID) && it.Kind == engine.HistoryText {
				ret, rerr := b.RetainHistoryContent(ctx, it.ID)
				if rerr != nil || len(ret.Content.Parts) == 0 {
					continue
				}
				rc, oerr := b.ReadContent(ctx, ret.LeaseID, ret.Content.Parts[0].ID)
				if oerr != nil {
					_ = b.ReleaseContent(ctx, ret.LeaseID)
					continue
				}
				data, _ := io.ReadAll(rc)
				_ = rc.Close()
				_ = b.ReleaseContent(ctx, ret.LeaseID)
				if string(data) == "instant-via-e2ee-relay" {
					return true
				}
			}
		}
		return false
	})
	waitUntil(t, 3*time.Second, "relay server cleaned single transfer", func() bool {
		return srv.ActiveTransferCount() == 0
	})

	// 2b. 0-wait multi-chunk (> 2 MiB) file transfer over E2EE HTTP relay.
	fileBytes := bytes.Repeat([]byte("unic-hybrid-relay-chunk-test-0123456789!"), (2<<20)/40+5000) // ~2.2 MiB -> 3 chunks
	wantHash := sha256.Sum256(fileBytes)
	tmpFile := filepath.Join(t.TempDir(), "sample.bin")
	if err := os.WriteFile(tmpFile, fileBytes, 0o600); err != nil {
		t.Fatal(err)
	}
	leaseFile, err := a.ImportContent(ctx, engine.ContentInput{
		Files: []engine.FileInput{{Name: "sample.bin", Size: int64(len(fileBytes)), Source: &memFileSource{path: tmpFile, size: int64(len(fileBytes))}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	resFile, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-relay-multi",
		LeaseID:   leaseFile.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil || resFile.SyncDisposition != engine.SyncQueued {
		t.Fatalf("relay multi commit failed: res=%+v err=%v", resFile, err)
	}
	waitUntil(t, 10*time.Second, "relay multi op complete", func() bool {
		op, err := a.GetOperation(ctx, resFile.OperationID)
		if err != nil || !op.Done() || len(op.Targets) != 1 {
			return false
		}
		tg := op.Targets[0]
		return tg.State == engine.TargetSent && tg.TransportRoute == engine.RouteRelay && tg.ReceiptStage == engine.ReceiptStageRelayStored
	})

	// Verify Mac assembled and committed the multi-chunk file with identical SHA-256.
	waitUntil(t, 10*time.Second, "Mac received multi-chunk file", func() bool {
		page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
		if err != nil {
			return false
		}
		for _, it := range page.Items {
			if it.Kind != engine.HistoryFiles || it.Origin.DeviceID != engine.DeviceID(aID) {
				continue
			}
			ret, rerr := b.RetainHistoryContent(ctx, it.ID)
			if rerr != nil || len(ret.Content.Parts) == 0 {
				continue
			}
			rc, oerr := b.ReadContent(ctx, ret.LeaseID, ret.Content.Parts[0].ID)
			if oerr != nil {
				_ = b.ReleaseContent(ctx, ret.LeaseID)
				continue
			}
			got, _ := io.ReadAll(rc)
			_ = rc.Close()
			_ = b.ReleaseContent(ctx, ret.LeaseID)
			if sha256.Sum256(got) == wantHash {
				return true
			}
		}
		return false
	})
	waitUntil(t, 3*time.Second, "relay server cleaned multi transfer", func() bool {
		return srv.ActiveTransferCount() == 0
	})

	// 3. Unpair Phone on Mac: Mac revokes Phone's sender token on relay server,
	// so Phone can no longer upload to Mac's mailbox.
	dsB, _ := b.ListDevices(ctx)
	var phoneDev engine.Device
	for _, d := range dsB {
		if d.ID == aID {
			phoneDev = d
		}
	}
	if err := b.UnpairDevice(ctx, engine.UnpairDeviceRequest{ID: phoneDev.ID, ExpectedRevision: phoneDev.Revision}); err != nil {
		t.Fatalf("UnpairDevice: %v", err)
	}
	blockedText := []byte("after-unpair")
	leaseBlocked, _ := a.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(blockedText)), Reader: bytes.NewReader(blockedText)}},
	})
	resBlocked, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-after-unpair",
		LeaseID:   leaseBlocked.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil {
		t.Fatalf("commit after unpair: %v", err)
	}
	if resBlocked.OperationID != "" {
		waitUntil(t, 5*time.Second, "revoked relay upload fails", func() bool {
			op, err := a.GetOperation(ctx, resBlocked.OperationID)
			return err == nil && op.Done() && len(op.Targets) == 1 && op.Targets[0].State == engine.TargetFailed
		})
	}
}

func TestRelayDisabledHasZeroHTTPRequests(t *testing.T) {
	srv := relayserver.New(relayserver.Config{})
	ts := httptest.NewServer(srv)
	defer func() {
		srv.Close()
		ts.Close()
	}()
	trA := newToggleTransport()
	_ = startRelayTestEngine(t, "NoRelay", ts.URL, false, 0, trA)
	time.Sleep(100 * time.Millisecond)
	if got := srv.RequestCount(); got != 0 {
		t.Fatalf("expected 0 relay HTTP requests when relay disabled, got %d", got)
	}
}

func TestHybridRelayMidReceiveDiscardDeletesServerTransfer(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{MaxTransferBytes: 10 << 20})
	chunk1Started := make(chan struct{})
	chunk1Release := make(chan struct{})
	var once sync.Once
	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && strings.HasSuffix(r.URL.Path, "/chunks/1") {
			once.Do(func() { close(chunk1Started) })
			select {
			case <-chunk1Release:
			case <-r.Context().Done():
				return
			}
		}
		srv.ServeHTTP(w, r)
	})
	ts := httptest.NewServer(handler)
	defer func() {
		select {
		case <-chunk1Release:
		default:
			close(chunk1Release)
		}
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	a := startRelayTestEngine(t, "PhoneDiscard", ts.URL, true, 10<<20, trA)
	b := startRelayTestEngine(t, "MacDiscard", ts.URL, true, 10<<20, trB)
	aID, bID := pairAndConnect(t, a, b)

	waitUntil(t, 5*time.Second, "relay route ready", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p offline", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	// Send a > 2 MiB file (3 chunks) so receiver downloads chunk 0 then stalls on chunk 1.
	payload := bytes.Repeat([]byte("discard-me-mid-stream-0123456789abcdefgh"), (2<<20)/40+4000)
	tmpFile := filepath.Join(t.TempDir(), "discard.bin")
	if err := os.WriteFile(tmpFile, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	lease, err := a.ImportContent(ctx, engine.ContentInput{
		Files: []engine.FileInput{{Name: "discard.bin", Size: int64(len(payload)), Source: &memFileSource{path: tmpFile, size: int64(len(payload))}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-relay-discard",
		LeaseID:   lease.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	}); err != nil {
		t.Fatal(err)
	}

	select {
	case <-chunk1Started:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for receiver to start chunk 1 download")
	}

	var recvOpID engine.OperationID
	waitUntil(t, 5*time.Second, "active receive operation on Mac", func() bool {
		ops, err := b.ListOperations(ctx)
		if err != nil {
			return false
		}
		for _, op := range ops {
			if op.Kind == engine.OperationReceive && op.Receive != nil && op.Receive.PeerID == engine.DeviceID(aID) && op.Receive.State == engine.ReceiveReceiving {
				recvOpID = op.ID
				return true
			}
		}
		return false
	})

	if err := b.DiscardReceive(ctx, recvOpID); err != nil {
		t.Fatalf("DiscardReceive: %v", err)
	}
	close(chunk1Release)

	waitUntil(t, 5*time.Second, "receive op marked discarded", func() bool {
		op, err := b.GetOperation(ctx, recvOpID)
		return err == nil && op.Receive != nil && op.Receive.State == engine.ReceiveDiscarded
	})
	waitUntil(t, 5*time.Second, "relay server deleted discarded transfer", func() bool {
		return srv.ActiveTransferCount() == 0
	})
	page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	for _, it := range page.Items {
		if it.Kind == engine.HistoryFiles && it.Origin.DeviceID == engine.DeviceID(aID) {
			t.Fatalf("discarded relay file unexpectedly committed to history: %+v", it)
		}
	}
}

func TestHybridRelayRejectsOversizedOfflinePayload(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{MaxTransferBytes: 10 << 20})
	ts := httptest.NewServer(srv)
	defer func() {
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	// Configure small 64 KiB relay cap on Phone.
	a := startRelayTestEngine(t, "PhoneCap", ts.URL, true, 64<<10, trA)
	b := startRelayTestEngine(t, "MacCap", ts.URL, true, 10<<20, trB)
	_, bID := pairAndConnect(t, a, b)

	waitUntil(t, 5*time.Second, "relay route ready", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p offline", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	bigText := bytes.Repeat([]byte("A"), 200<<10) // 200 KiB > 64 KiB relay cap
	lease, err := a.ImportContent(ctx, engine.ContentInput{
		Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(bigText)), Reader: bytes.NewReader(bigText)}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-relay-oversized",
		LeaseID:   lease.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	if res.OperationID != "" {
		waitUntil(t, 5*time.Second, "oversized relay op fails with limit_exceeded", func() bool {
			op, err := a.GetOperation(ctx, res.OperationID)
			if err != nil || !op.Done() || len(op.Targets) != 1 {
				return false
			}
			tg := op.Targets[0]
			return tg.State == engine.TargetFailed && tg.Error != nil && tg.Error.Code == engine.CodeLimitExceeded
		})
	}
	if srv.ActiveTransferCount() != 0 {
		t.Fatalf("expected 0 relay transfers for oversized payload, got %d", srv.ActiveTransferCount())
	}
}

func TestHybridRelayRejectsMismatchedPeerOrigin(t *testing.T) {
	ctx := context.Background()
	srv1 := relayserver.New(relayserver.Config{})
	srv2 := relayserver.New(relayserver.Config{})
	ts1 := httptest.NewServer(srv1)
	ts2 := httptest.NewServer(srv2)
	defer func() {
		srv1.Close()
		srv2.Close()
		ts1.Close()
		ts2.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	// Phone trusts ts1; Mac advertises a mailbox on ts2 -> Phone must reject the mismatched origin.
	a := startRelayTestEngine(t, "PhoneOrigin", ts1.URL, true, 10<<20, trA)
	b := startRelayTestEngine(t, "MacOtherOrigin", ts2.URL, true, 10<<20, trB)
	_, bID := pairAndConnect(t, a, b)

	// Allow time for relay_profile frame to arrive and be rejected.
	time.Sleep(300 * time.Millisecond)
	ds, err := a.ListDevices(ctx)
	if err != nil {
		t.Fatal(err)
	}
	for _, d := range ds {
		if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
			t.Fatalf("expected peer with mismatched relay origin to be rejected, but RelayRouteReady=true")
		}
	}
}

func TestHybridRelayLevel3MidTransferP2PHandoff(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{MaxTransferBytes: 20 << 20})

	var httpPutChunks atomic.Int32
	chunk2Gate := make(chan struct{})
	chunk2Reached := make(chan struct{})
	var gateOnce sync.Once
	var releaseOnce sync.Once
	releaseGate := func() { releaseOnce.Do(func() { close(chunk2Gate) }) }

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPut && strings.Contains(r.URL.Path, "/chunks/") {
			n := httpPutChunks.Add(1)
			if n >= 3 {
				gateOnce.Do(func() { close(chunk2Reached) })
				// Hold HTTP uploads beyond first 2 chunks so P2P recovery triggers Level 3 handoff.
				select {
				case <-chunk2Gate:
				case <-r.Context().Done():
					return
				}
			}
		}
		srv.ServeHTTP(w, r)
	})
	ts := httptest.NewServer(handler)
	defer func() {
		releaseGate()
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	a := startRelayTestEngine(t, "PhoneHandoff", ts.URL, true, 20<<20, trA)
	b := startRelayTestEngine(t, "MacHandoff", ts.URL, true, 20<<20, trB)
	aID, bID := pairAndConnect(t, a, b)

	waitUntil(t, 5*time.Second, "relay route ready", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	// Block P2P so transfer starts immediately via HTTP relay.
	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p offline", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	// 8 MiB file -> 8 chunks of 1 MiB.
	payload := bytes.Repeat([]byte("level3-handoff-chunked-transfer-01234567"), (8<<20)/40)
	wantHash := sha256.Sum256(payload)
	tmpFile := filepath.Join(t.TempDir(), "handoff8m.bin")
	if err := os.WriteFile(tmpFile, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	lease, err := a.ImportContent(ctx, engine.ContentInput{
		Files: []engine.FileInput{{Name: "handoff8m.bin", Size: int64(len(payload)), Source: &memFileSource{path: tmpFile, size: int64(len(payload))}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-level3-handoff",
		LeaseID:   lease.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil || res.SyncDisposition != engine.SyncQueued {
		t.Fatalf("CommitContent: res=%+v err=%v", res, err)
	}

	// Wait until initial chunks are uploaded to HTTP and upload pauses on chunk 3+.
	select {
	case <-chunk2Reached:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for HTTP relay upload to start")
	}

	// Restore P2P mid-transfer: sender settles via relay_handoff_offer/ack and finishes over P2P.
	trA.SetBlocked(false)
	trB.SetBlocked(false)
	// Trigger immediate reconnect without waiting for backoff.
	_ = a.NotifyNetworkChanged(ctx)
	_ = b.NotifyNetworkChanged(ctx)

	waitUntil(t, 15*time.Second, "hybrid handoff op completes with remote_received", func() bool {
		op, err := a.GetOperation(ctx, res.OperationID)
		if err != nil || !op.Done() || len(op.Targets) != 1 {
			return false
		}
		tg := op.Targets[0]
		return tg.State == engine.TargetSent &&
			tg.TransportRoute == engine.RouteHybrid &&
			tg.ReceiptStage == engine.ReceiptStageRemoteReceived
	})
	releaseGate()

	// Verify HTTP did NOT upload all 8 chunks (remaining chunks went via P2P).
	if puts := httpPutChunks.Load(); puts >= 8 {
		t.Fatalf("expected P2P handoff to avoid uploading all 8 chunks over HTTP, got %d HTTP PUTs", puts)
	}

	// Verify Mac committed the 8 MiB file EXACTLY ONCE with identical SHA-256.
	waitUntil(t, 5*time.Second, "Mac committed handoff file", func() bool {
		page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
		if err != nil {
			return false
		}
		matches := 0
		for _, it := range page.Items {
			if it.Kind == engine.HistoryFiles && it.Origin.DeviceID == engine.DeviceID(aID) {
				matches++
				ret, rerr := b.RetainHistoryContent(ctx, it.ID)
				if rerr != nil || len(ret.Content.Parts) == 0 {
					continue
				}
				rc, oerr := b.ReadContent(ctx, ret.LeaseID, ret.Content.Parts[0].ID)
				if oerr != nil {
					_ = b.ReleaseContent(ctx, ret.LeaseID)
					continue
				}
				got, _ := io.ReadAll(rc)
				_ = rc.Close()
				_ = b.ReleaseContent(ctx, ret.LeaseID)
				if sha256.Sum256(got) != wantHash {
					t.Fatalf("handoff file SHA-256 mismatch")
				}
			}
		}
		if matches > 1 {
			t.Fatalf("duplicate history items committed: %d", matches)
		}
		return matches == 1
	})

	waitUntil(t, 5*time.Second, "relay server cleaned handoff transfer", func() bool {
		return srv.ActiveTransferCount() == 0
	})
}

func TestHybridRelayP2PDropMidHandoffReverseFallbackToHTTP(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{MaxTransferBytes: 20 << 20})

	var httpPutChunks atomic.Int32
	gate := make(chan struct{})
	gateReached := make(chan struct{})
	var gateOnce, releaseOnce sync.Once
	releaseGate := func() { releaseOnce.Do(func() { close(gate) }) }

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPut && strings.Contains(r.URL.Path, "/chunks/") {
			if httpPutChunks.Add(1) >= 3 {
				gateOnce.Do(func() { close(gateReached) })
				select {
				case <-gate:
				case <-r.Context().Done():
					return
				}
			}
		}
		srv.ServeHTTP(w, r)
	})
	ts := httptest.NewServer(handler)
	defer func() {
		releaseGate()
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	a := startRelayTestEngine(t, "PhoneFallback", ts.URL, true, 20<<20, trA)
	b := startRelayTestEngine(t, "MacFallback", ts.URL, true, 20<<20, trB)
	aID, bID := pairAndConnect(t, a, b)

	waitUntil(t, 5*time.Second, "relay route ready", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p offline", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	// 8 MiB file -> 8 chunks.
	payload := bytes.Repeat([]byte("reverse-fallback-after-p2p-cut-012345678"), (8<<20)/40)
	wantHash := sha256.Sum256(payload)
	tmpFile := filepath.Join(t.TempDir(), "fallback8m.bin")
	if err := os.WriteFile(tmpFile, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	lease, err := a.ImportContent(ctx, engine.ContentInput{
		Files: []engine.FileInput{{Name: "fallback8m.bin", Size: int64(len(payload)), Source: &memFileSource{path: tmpFile, size: int64(len(payload))}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-reverse-fallback",
		LeaseID:   lease.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil || res.SyncDisposition != engine.SyncQueued {
		t.Fatalf("CommitContent: res=%+v err=%v", res, err)
	}

	select {
	case <-gateReached:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for initial HTTP chunks")
	}

	// Cut P2P automatically once Phone has written ~2.5 MiB over the recovered P2P session!
	var p2pWritten atomic.Int64
	var cutOnce sync.Once
	p2pCut := make(chan struct{})
	trA.SetWriteHook(func(n int) {
		if p2pWritten.Add(int64(n)) >= 2500*1024 {
			cutOnce.Do(func() {
				// Release HTTP gate so reverse fallback completes immediately over HTTP.
				releaseGate()
				trA.SetBlocked(true)
				trB.SetBlocked(true)
				close(p2pCut)
			})
		}
	})

	// Unblock P2P to trigger handoff; once ~2.5 MiB flows over P2P, hook cuts P2P.
	trA.SetBlocked(false)
	trB.SetBlocked(false)
	_ = a.NotifyNetworkChanged(ctx)
	_ = b.NotifyNetworkChanged(ctx)

	select {
	case <-p2pCut:
	case <-time.After(15 * time.Second):
		t.Fatal("timed out waiting for mid-handoff P2P cut")
	}

	// Sender must reverse-fallback cleanly to HTTP and finish with RouteHybrid + ReceiptStageRelayStored.
	waitUntil(t, 15*time.Second, "reverse-fallback op finishes cleanly", func() bool {
		op, err := a.GetOperation(ctx, res.OperationID)
		if err != nil || !op.Done() || len(op.Targets) != 1 {
			return false
		}
		tg := op.Targets[0]
		return tg.State == engine.TargetSent &&
			tg.TransportRoute == engine.RouteHybrid &&
			tg.ReceiptStage == engine.ReceiptStageRelayStored
	})

	// Receiver must assemble chunks across HTTP + P2P + reverse-fallback HTTP with identical SHA-256 and 0 duplicates.
	waitUntil(t, 15*time.Second, "Mac assembled file after reverse fallback", func() bool {
		page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
		if err != nil {
			return false
		}
		matches := 0
		for _, it := range page.Items {
			if it.Kind == engine.HistoryFiles && it.Origin.DeviceID == engine.DeviceID(aID) {
				matches++
				ret, rerr := b.RetainHistoryContent(ctx, it.ID)
				if rerr != nil || len(ret.Content.Parts) == 0 {
					continue
				}
				rc, oerr := b.ReadContent(ctx, ret.LeaseID, ret.Content.Parts[0].ID)
				if oerr != nil {
					_ = b.ReleaseContent(ctx, ret.LeaseID)
					continue
				}
				got, _ := io.ReadAll(rc)
				_ = rc.Close()
				_ = b.ReleaseContent(ctx, ret.LeaseID)
				if sha256.Sum256(got) != wantHash {
					t.Fatalf("reverse-fallback file SHA-256 mismatch")
				}
			}
		}
		if matches > 1 {
			t.Fatalf("duplicate history items committed after reverse fallback: %d", matches)
		}
		return matches == 1
	})

	waitUntil(t, 5*time.Second, "relay server cleaned transfer after reverse fallback", func() bool {
		return srv.ActiveTransferCount() == 0
	})
}

func TestHybridRelayMidHandoffDiscardStopsSender(t *testing.T) {
	ctx := context.Background()
	srv := relayserver.New(relayserver.Config{MaxTransferBytes: 20 << 20})

	var httpPutChunks atomic.Int32
	gate := make(chan struct{})
	gateReached := make(chan struct{})
	var gateOnce, releaseOnce sync.Once
	releaseGate := func() { releaseOnce.Do(func() { close(gate) }) }

	handler := http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPut && strings.Contains(r.URL.Path, "/chunks/") {
			if httpPutChunks.Add(1) >= 3 {
				gateOnce.Do(func() { close(gateReached) })
				select {
				case <-gate:
				case <-r.Context().Done():
					return
				}
			}
		}
		srv.ServeHTTP(w, r)
	})
	ts := httptest.NewServer(handler)
	defer func() {
		releaseGate()
		srv.Close()
		ts.Close()
	}()

	trA, trB := newToggleTransport(), newToggleTransport()
	a := startRelayTestEngine(t, "PhoneHandoffDiscard", ts.URL, true, 20<<20, trA)
	b := startRelayTestEngine(t, "MacHandoffDiscard", ts.URL, true, 20<<20, trB)
	aID, bID := pairAndConnect(t, a, b)

	waitUntil(t, 5*time.Second, "relay route ready", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].RelayRouteReady {
				return true
			}
		}
		return false
	})

	trA.SetBlocked(true)
	trB.SetBlocked(true)
	waitUntil(t, 5*time.Second, "p2p offline", func() bool {
		ds, _ := a.ListDevices(ctx)
		for _, d := range ds {
			if d.ID == bID && len(d.Bindings) > 0 && d.Bindings[0].Connection != engine.Connected {
				return true
			}
		}
		return false
	})

	payload := bytes.Repeat([]byte("discard-during-p2p-handoff-0123456789abc"), (8<<20)/40)
	tmpFile := filepath.Join(t.TempDir(), "discardhandoff8m.bin")
	if err := os.WriteFile(tmpFile, payload, 0o600); err != nil {
		t.Fatal(err)
	}
	lease, err := a.ImportContent(ctx, engine.ContentInput{
		Files: []engine.FileInput{{Name: "discardhandoff8m.bin", Size: int64(len(payload)), Source: &memFileSource{path: tmpFile, size: int64(len(payload))}}},
	})
	if err != nil {
		t.Fatal(err)
	}
	res, err := a.CommitContent(ctx, engine.CommitRequest{
		RequestID: "req-handoff-discard",
		LeaseID:   lease.LeaseID,
		DeviceIDs: []engine.DeviceID{engine.DeviceID(bID)},
	})
	if err != nil || res.SyncDisposition != engine.SyncQueued {
		t.Fatalf("CommitContent: res=%+v err=%v", res, err)
	}

	select {
	case <-gateReached:
	case <-time.After(10 * time.Second):
		t.Fatal("timed out waiting for initial HTTP chunks")
	}

	// Hold P2P chunk writes after ~1.5 MiB until we call DiscardReceive on Mac!
	var p2pWritten atomic.Int64
	p2pPaused := make(chan struct{})
	p2pResume := make(chan struct{})
	var pauseOnce, resumeOnce sync.Once
	resumeP2P := func() { resumeOnce.Do(func() { close(p2pResume) }) }
	defer resumeP2P()

	trA.SetWriteHook(func(n int) {
		if p2pWritten.Add(int64(n)) >= 1500*1024 {
			pauseOnce.Do(func() { close(p2pPaused) })
			<-p2pResume
		}
	})

	trA.SetBlocked(false)
	trB.SetBlocked(false)
	_ = a.NotifyNetworkChanged(ctx)
	_ = b.NotifyNetworkChanged(ctx)

	select {
	case <-p2pPaused:
	case <-time.After(15 * time.Second):
		t.Fatal("timed out waiting for P2P handoff chunk stream")
	}

	// Find active receive operation on Mac and discard it mid-handoff.
	var recvOpID engine.OperationID
	waitUntil(t, 5*time.Second, "active receive op on Mac", func() bool {
		ops, err := b.ListOperations(ctx)
		if err != nil {
			return false
		}
		for _, op := range ops {
			if op.Kind == engine.OperationReceive && op.Receive != nil && op.Receive.PeerID == engine.DeviceID(aID) && op.Receive.State == engine.ReceiveReceiving {
				recvOpID = op.ID
				return true
			}
		}
		return false
	})
	if err := b.DiscardReceive(ctx, recvOpID); err != nil {
		t.Fatalf("DiscardReceive: %v", err)
	}
	resumeP2P()
	releaseGate()

	// Phone must stop and transition to TargetDiscarded + ReceiptStageRemoteDiscarded + RouteHybrid.
	waitUntil(t, 10*time.Second, "sender observes remote_discarded during handoff", func() bool {
		op, err := a.GetOperation(ctx, res.OperationID)
		if err != nil || !op.Done() || len(op.Targets) != 1 {
			return false
		}
		tg := op.Targets[0]
		return tg.State == engine.TargetDiscarded &&
			tg.ReceiptStage == engine.ReceiptStageRemoteDiscarded &&
			tg.TransportRoute == engine.RouteHybrid
	})

	waitUntil(t, 5*time.Second, "relay server deleted discarded handoff transfer", func() bool {
		return srv.ActiveTransferCount() == 0
	})
	page, err := b.ListHistory(ctx, engine.ListHistoryRequest{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	for _, it := range page.Items {
		if it.Kind == engine.HistoryFiles && it.Origin.DeviceID == engine.DeviceID(aID) {
			t.Fatalf("discarded handoff file unexpectedly committed to history: %+v", it)
		}
	}
}

type memFileSource struct {
	path string
	size int64
}

func (m *memFileSource) Open(context.Context) (io.ReadCloser, engine.FileMeta, error) {
	f, err := os.Open(m.path)
	if err != nil {
		return nil, engine.FileMeta{}, err
	}
	return f, engine.FileMeta{Size: m.size, Version: "v1"}, nil
}
