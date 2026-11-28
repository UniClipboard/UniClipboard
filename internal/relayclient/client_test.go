package relayclient_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
	"github.com/uniclipboard/unic/internal/relayserver"
)

func TestRelayClientServerDirectToS3RoundTripAuthAndRefresh(t *testing.T) {
	sender, _ := identity.FromSeed(bytes.Repeat([]byte{0x11}, 32))
	recipient, _ := identity.FromSeed(bytes.Repeat([]byte{0x22}, 32))
	outsider, _ := identity.FromSeed(bytes.Repeat([]byte{0x33}, 32))

	accessKey := "self-host-secret-key"
	allowed := map[string]struct{}{
		base64.RawURLEncoding.EncodeToString(recipient.Public()): {},
	}

	// Mount simulated S3 storage on a SEPARATE server to prove credentials are
	// stripped and cross-origin presigned transfers work cleanly.
	var storageCalls atomic.Int32
	var srv *relayserver.Server
	storageTS := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		storageCalls.Add(1)
		// Double-check no relay credentials ever reach S3.
		if r.Header.Get("Authorization") != "" || r.Header.Get("X-Unic-Relay-Key") != "" || r.Header.Get("X-Unic-Sig") != "" {
			t.Errorf("relay credentials leaked to S3 storage: %+v", r.Header)
			w.WriteHeader(http.StatusBadRequest)
			return
		}
		srv.StorageHandler().ServeHTTP(w, r)
	}))
	defer storageTS.Close()

	now := time.Now().Truncate(time.Second)
	var clockMu atomic.Int64
	clockMu.Store(now.UnixNano())
	clock := func() time.Time { return time.Unix(0, clockMu.Load()) }

	srv = relayserver.New(relayserver.Config{
		StorageBaseURL:      storageTS.URL,
		AccessKey:           accessKey,
		AllowedPubKeys:      allowed,
		MaxInlineUploadURLs: 2, // Force chunks >= 2 to exercise POST .../upload-url
		PresignTTL:          30 * time.Second,
		Now:                 clock,
	})
	ts := httptest.NewServer(srv)
	defer func() {
		srv.Close()
		ts.Close()
	}()

	cli, err := relayclient.New(relayclient.Config{
		BaseURL:    ts.URL,
		AccessKey:  accessKey,
		HTTPClient: ts.Client(),
		SSEClient:  ts.Client(),
		Now:        clock,
	})
	if err != nil {
		t.Fatal(err)
	}
	mb, err := relaycrypto.DeriveMailboxID(cli.Origin(), recipient.Public())
	if err != nil {
		t.Fatal(err)
	}

	// Wrong access key rejected.
	badKeyCli, _ := relayclient.New(relayclient.Config{BaseURL: ts.URL, AccessKey: "wrong", HTTPClient: ts.Client(), Now: clock})
	if _, err := badKeyCli.ListTransfers(context.Background(), recipient, mb); err == nil {
		t.Fatalf("expected error with wrong access key")
	}

	// Outsider pubkey rejected by AllowedPubKeys.
	outsiderMB, _ := relaycrypto.DeriveMailboxID(cli.Origin(), outsider.Public())
	if _, err := cli.ListTransfers(context.Background(), outsider, outsiderMB); err == nil {
		t.Fatalf("expected error for disallowed pubkey")
	}

	// Register sender delivery token.
	token, err := relaycrypto.NewDeliveryToken()
	if err != nil {
		t.Fatal(err)
	}
	tokenHash := relaycrypto.TokenHash(token)
	if err := cli.RegisterSender(context.Background(), recipient, mb, tokenHash, 10<<20); err != nil {
		t.Fatalf("RegisterSender: %v", err)
	}

	// Subscribe SSE in background.
	sseCtx, sseCancel := context.WithCancel(context.Background())
	defer sseCancel()
	evtCh := make(chan relayclient.SSEEvent, 16)
	go func() {
		_ = cli.SubscribeEvents(sseCtx, recipient, mb, "", func(ev relayclient.SSEEvent) error {
			evtCh <- ev
			return nil
		})
	}()
	time.Sleep(50 * time.Millisecond)

	// 1. Single-chunk transfer via unified Direct-to-S3 CreateTransfer -> PUT -> commit -> GET -> ack.
	tid1 := "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
	cek1 := bytes.Repeat([]byte{0x55}, 32)
	cc1, _ := relaycrypto.NewChunkCipher(cek1, tid1, sender.DeviceID(), recipient.DeviceID(), 1)
	ct0, h0, err := cc1.SealChunk(0, []byte("instant mobile share payload"))
	if err != nil {
		t.Fatal(err)
	}
	env1, err := relaycrypto.SealEnvelope(sender, relaycrypto.SealEnvelopeInput{
		RelayOrigin:          cli.Origin(),
		TransferID:           tid1,
		RecipientPK:          recipient.Public(),
		CreatedMs:            now.UnixMilli(),
		ExpiresMs:            now.Add(relaycrypto.DefaultTTL).UnixMilli(),
		ChunkSize:            len("instant mobile share payload"),
		TotalChunks:          1,
		TotalCiphertextBytes: int64(len(ct0)),
		ChunkHashes:          [][32]byte{h0},
		CEK:                  cek1,
	})
	if err != nil {
		t.Fatal(err)
	}
	cr1, err := cli.CreateTransfer(context.Background(), mb, token, env1)
	if err != nil {
		t.Fatalf("CreateTransfer(single): %v", err)
	}
	if cr1.UploadMode != "presigned" || len(cr1.UploadURLs) != 1 {
		t.Fatalf("unexpected CreateTransfer response: %+v", cr1)
	}
	if err := cli.UploadChunk(context.Background(), mb, token, tid1, 0, ct0, &cr1.UploadURLs[0]); err != nil {
		t.Fatalf("UploadChunk(single): %v", err)
	}

	// Expect transfer_announced followed by chunk_ready for index 0.
	var sawAnnounced, sawReady bool
	for !sawAnnounced || !sawReady {
		select {
		case ev := <-evtCh:
			if ev.TransferID != tid1 {
				continue
			}
			if ev.Event == "transfer_announced" {
				sawAnnounced = true
			}
			if ev.Event == "chunk_ready" && ev.ChunkIndex != nil && *ev.ChunkIndex == 0 {
				sawReady = true
			}
		case <-time.After(3 * time.Second):
			t.Fatalf("timed out waiting for SSE events (announced=%v ready=%v)", sawAnnounced, sawReady)
		}
	}

	gotCT0, err := cli.DownloadChunk(context.Background(), recipient, mb, tid1, 0, 1<<20)
	if err != nil || !bytes.Equal(gotCT0, ct0) {
		t.Fatalf("DownloadChunk(0): err=%v", err)
	}
	if srv.StoredObjectCount() != 1 {
		t.Fatalf("expected 1 stored S3 object before ack, got %d", srv.StoredObjectCount())
	}
	if err := cli.AckTransfer(context.Background(), recipient, mb, tid1); err != nil {
		t.Fatalf("AckTransfer: %v", err)
	}
	if srv.ActiveTransferCount() != 0 || srv.StoredObjectCount() != 0 {
		t.Fatalf("expected 0 transfers/objects after ack, got transfers=%d objects=%d", srv.ActiveTransferCount(), srv.StoredObjectCount())
	}

	// 2. Multi-chunk upload (4 chunks > MaxInlineUploadURLs=2) + expired grant auto-refresh + delete cleanup.
	tid2 := "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	cek2 := bytes.Repeat([]byte{0x66}, 32)
	cc2, _ := relaycrypto.NewChunkCipher(cek2, tid2, sender.DeviceID(), recipient.DeviceID(), 4)
	cts2 := make([][]byte, 4)
	hashes2 := make([][32]byte, 4)
	var totalCT2 int64
	for i := 0; i < 4; i++ {
		sz := relaycrypto.MinChunkSize
		if i == 3 {
			sz = 1234
		}
		pt := bytes.Repeat([]byte{byte('A' + i)}, sz)
		ct, h, cerr := cc2.SealChunk(i, pt)
		if cerr != nil {
			t.Fatal(cerr)
		}
		cts2[i], hashes2[i] = ct, h
		totalCT2 += int64(len(ct))
	}
	env2, err := relaycrypto.SealEnvelope(sender, relaycrypto.SealEnvelopeInput{
		RelayOrigin:          cli.Origin(),
		TransferID:           tid2,
		RecipientPK:          recipient.Public(),
		CreatedMs:            now.UnixMilli(),
		ExpiresMs:            now.Add(relaycrypto.DefaultTTL).UnixMilli(),
		ChunkSize:            relaycrypto.MinChunkSize,
		TotalChunks:          4,
		TotalCiphertextBytes: totalCT2,
		ChunkHashes:          hashes2,
		CEK:                  cek2,
	})
	if err != nil {
		t.Fatal(err)
	}
	cr2, err := cli.CreateTransfer(context.Background(), mb, token, env2)
	if err != nil {
		t.Fatalf("CreateTransfer(multi): %v", err)
	}
	if len(cr2.UploadURLs) != 2 {
		t.Fatalf("expected 2 inline upload grants, got %d", len(cr2.UploadURLs))
	}
	// Chunk 0 uses inline grant directly.
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 0, cts2[0], &cr2.UploadURLs[0]); err != nil {
		t.Fatalf("UploadChunk(0): %v", err)
	}
	// Advance clock past PresignTTL (30s) so chunk 1's inline grant is expired on server;
	// also spoof ExpiresAtMs far in the future to exercise reactive 403 refresh!
	clockMu.Store(now.Add(45 * time.Second).UnixNano())
	staleGrant := cr2.UploadURLs[1]
	staleGrant.ExpiresAtMs = now.Add(10 * time.Minute).UnixMilli()
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 1, cts2[1], &staleGrant); err != nil {
		t.Fatalf("UploadChunk(1, stale grant refresh): %v", err)
	}
	// Chunks 2 & 3 have no inline grant -> must fetch upload-url on demand.
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 2, cts2[2], nil); err != nil {
		t.Fatalf("UploadChunk(2): %v", err)
	}
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 3, cts2[3], nil); err != nil {
		t.Fatalf("UploadChunk(3): %v", err)
	}
	if srv.StoredObjectCount() != 4 {
		t.Fatalf("expected 4 stored S3 objects, got %d", srv.StoredObjectCount())
	}
	list, err := cli.ListTransfers(context.Background(), recipient, mb)
	if err != nil || len(list) != 1 || list[0].TransferID != tid2 {
		t.Fatalf("ListTransfers: err=%v list=%+v", err, list)
	}
	// Download chunk 3 directly from S3 and verify bytes match.
	gotC3, err := cli.DownloadChunk(context.Background(), recipient, mb, tid2, 3, 1<<20)
	if err != nil || !bytes.Equal(gotC3, cts2[3]) {
		t.Fatalf("DownloadChunk(3): err=%v", err)
	}
	if err := cli.DeleteTransferAsOwner(context.Background(), recipient, mb, tid2); err != nil {
		t.Fatalf("DeleteTransferAsOwner: %v", err)
	}
	if srv.ActiveTransferCount() != 0 || srv.StoredObjectCount() != 0 {
		t.Fatalf("expected 0 transfers/objects after delete, got transfers=%d objects=%d", srv.ActiveTransferCount(), srv.StoredObjectCount())
	}
	if storageCalls.Load() < 6 {
		t.Fatalf("expected direct S3 storage calls, got %d", storageCalls.Load())
	}

	// 3. Revoke sender token and verify subsequent CreateTransfer is forbidden.
	if err := cli.RevokeSender(context.Background(), recipient, mb, tokenHash); err != nil {
		t.Fatalf("RevokeSender: %v", err)
	}
	_, err = cli.CreateTransfer(context.Background(), mb, token, env1)
	var re *relayclient.Error
	if !errors.As(err, &re) || re.Code != "permission_denied" {
		t.Fatalf("expected permission_denied after revoke, got %v", err)
	}

	// 4. SSRF origin check.
	if err := cli.ValidatePeerOrigin("https://127.0.0.1:9999"); err == nil {
		t.Fatalf("expected origin mismatch error")
	}
}

func TestRelayClientPresignedURLSSRFProtectionAndRedaction(t *testing.T) {
	recipient, _ := identity.FromSeed(bytes.Repeat([]byte{0x44}, 32))

	badURLs := []string{
		"http://bucket.s3.example.com/key?X-Amz-Signature=supersecret123",
		"https://127.0.0.1/key?X-Amz-Signature=supersecret123",
		"https://localhost/key?X-Amz-Signature=supersecret123",
		"https://[::1]/key?X-Amz-Signature=supersecret123",
		"https://169.254.169.254/latest/meta-data/?X-Amz-Signature=supersecret123",
		"https://10.0.0.1/key?X-Amz-Signature=supersecret123",
		"https://192.168.1.10/key?X-Amz-Signature=supersecret123",
		"https://100.64.0.1/key?X-Amz-Signature=supersecret123",
		"https://user:pass@bucket.s3.example.com/key?X-Amz-Signature=supersecret123",
		"file:///etc/passwd?X-Amz-Signature=supersecret123",
	}

	for _, bad := range badURLs {
		// Build a mock control-plane HTTP client for a PUBLIC relay origin (https://relay.uniclipboard.app)
		// so loopback/private storage URLs MUST be rejected.
		mockRT := roundTripFunc(func(req *http.Request) (*http.Response, error) {
			rec := httptest.NewRecorder()
			rec.Header().Set("Content-Type", "application/json")
			_ = json.NewEncoder(rec).Encode(map[string]any{
				"method":        "GET",
				"url":           bad,
				"expires_at_ms": time.Now().Add(time.Minute).UnixMilli(),
				"size":          16,
			})
			return rec.Result(), nil
		})
		cli, err := relayclient.New(relayclient.Config{
			BaseURL:    "https://relay.uniclipboard.app",
			HTTPClient: &http.Client{Transport: mockRT},
		})
		if err != nil {
			t.Fatal(err)
		}
		mb, _ := relaycrypto.DeriveMailboxID(cli.Origin(), recipient.Public())
		_, err = cli.DownloadChunk(context.Background(), recipient, mb, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", 0, 1024)
		var re *relayclient.Error
		if !errors.As(err, &re) || re.Code != "invalid_storage_url" {
			t.Fatalf("expected invalid_storage_url for %q, got %v", bad, err)
		}
		if strings.Contains(err.Error(), "supersecret123") || strings.Contains(err.Error(), "http") {
			t.Fatalf("presigned URL leaked into error string: %q", err.Error())
		}
	}
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

func TestControlAndSSESendBoundaryAdmissionAndInFlightSemantics(t *testing.T) {
	owner, _ := identity.FromSeed(bytes.Repeat([]byte{0x55}, 32))
	mb, err := relaycrypto.DeriveMailboxID("https://relay.example.com", owner.Public())
	if err != nil {
		t.Fatal(err)
	}

	var mu sync.Mutex
	curKey := "k1"
	curAllow := true
	getState := func() (string, bool) {
		mu.Lock()
		defer mu.Unlock()
		return curKey, curAllow
	}
	setState := func(k string, allow bool) {
		mu.Lock()
		curKey = k
		curAllow = allow
		mu.Unlock()
	}

	// 1. Control-plane & SSE 401 same-origin recovery, refusal, and pre-send revocation.
	type scenario int
	const (
		scRecover scenario = iota
		scAuthRefuseAllow
		scAuthRefuseDeny
		scRevokeBeforeRetrySend
		scUnchangedKeyBeforeRetrySend
		scRedirectOffOrigin
		scInFlightSurvivesRevoke
		scInFlight401RetryRevoked
	)

	var activeScenario scenario
	var hits atomic.Int32
	var secondaryHits atomic.Int32
	var presentedKeys []string
	var inFlightEntered chan struct{}
	var inFlightRelease chan struct{}

	rt := roundTripFunc(func(req *http.Request) (*http.Response, error) {
		if req.URL != nil && req.URL.Host == "other.example.com" {
			secondaryHits.Add(1)
		}
		hits.Add(1)
		mu.Lock()
		presentedKeys = append(presentedKeys, req.Header.Get("X-Unic-Relay-Key"))
		mu.Unlock()

		isSSE := strings.HasSuffix(req.URL.Path, "/events")
		rec := httptest.NewRecorder()
		switch activeScenario {
		case scRecover:
			if req.Header.Get("X-Unic-Relay-Key") == "k1" {
				rec.WriteHeader(http.StatusUnauthorized)
				return rec.Result(), nil
			}
			if isSSE {
				rec.Header().Set("Content-Type", "text/event-stream")
				rec.WriteHeader(http.StatusOK)
				_, _ = rec.WriteString("id: 1\nevent: transfer\ndata: {\"id\":\"1\",\"event\":\"transfer\"}\n\n")
				return rec.Result(), nil
			}
			rec.Header().Set("Content-Type", "application/json")
			_, _ = rec.WriteString(`{"transfers":[]}`)
			return rec.Result(), nil
		case scAuthRefuseAllow, scAuthRefuseDeny, scRevokeBeforeRetrySend, scUnchangedKeyBeforeRetrySend:
			rec.WriteHeader(http.StatusUnauthorized)
			return rec.Result(), nil
		case scRedirectOffOrigin:
			rec.Header().Set("Location", "https://other.example.com/v1/relay/stolen")
			rec.WriteHeader(http.StatusFound)
			return rec.Result(), nil
		case scInFlightSurvivesRevoke, scInFlight401RetryRevoked:
			mu.Lock()
			entered := inFlightEntered
			inFlightEntered = nil
			release := inFlightRelease
			mu.Unlock()
			if entered != nil {
				close(entered)
			}
			if release != nil {
				<-release
			}
			if activeScenario == scInFlight401RetryRevoked {
				rec.WriteHeader(http.StatusUnauthorized)
				return rec.Result(), nil
			}
			rec.Header().Set("Content-Type", "application/json")
			_, _ = rec.WriteString(`{"transfers":[]}`)
			return rec.Result(), nil
		}
		return rec.Result(), nil
	})

	var onAuthFailure func(context.Context, string) bool
	httpCli := &http.Client{Transport: rt}
	sseCli := &http.Client{Transport: rt}
	cli, err := relayclient.New(relayclient.Config{
		BaseURL:           "https://relay.example.com",
		HTTPClient:        httpCli,
		SSEClient:         sseCli,
		AccessKeyProvider: getState,
		OnAuthFailure: func(ctx context.Context, rejected string) bool {
			return onAuthFailure(ctx, rejected)
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	reset := func(sc scenario) {
		mu.Lock()
		presentedKeys = nil
		mu.Unlock()
		hits.Store(0)
		secondaryHits.Store(0)
		activeScenario = sc
		setState("k1", true)
	}

	// (a) Control-plane and SSE recover cleanly when 401 rotates k1 -> k2 and admission allows.
	reset(scRecover)
	onAuthFailure = func(_ context.Context, rejected string) bool {
		if rejected != "k1" {
			t.Errorf("expected rejected=k1, got %q", rejected)
		}
		setState("k2", true)
		return true
	}
	if _, err := cli.ListTransfers(context.Background(), owner, mb); err != nil {
		t.Fatalf("ListTransfers recover: %v", err)
	}
	if hits.Load() != 2 || len(presentedKeys) != 2 || presentedKeys[0] != "k1" || presentedKeys[1] != "k2" {
		t.Fatalf("unexpected control recover hits=%d keys=%v", hits.Load(), presentedKeys)
	}

	reset(scRecover)
	errStop := errors.New("stop sse")
	gotEvent := false
	err = cli.SubscribeEvents(context.Background(), owner, mb, "", func(ev relayclient.SSEEvent) error {
		if ev.ID == "1" {
			gotEvent = true
		}
		return errStop
	})
	if !errors.Is(err, errStop) || !gotEvent {
		t.Fatalf("SubscribeEvents recover: err=%v gotEvent=%v", err, gotEvent)
	}
	if hits.Load() != 2 || len(presentedKeys) != 2 || presentedKeys[0] != "k1" || presentedKeys[1] != "k2" {
		t.Fatalf("unexpected SSE recover hits=%d keys=%v", hits.Load(), presentedKeys)
	}

	// (b) OnAuthFailure false preserves permission_denied when allowed, unavailable when denied, zero retries.
	for _, sc := range []scenario{scAuthRefuseAllow, scAuthRefuseDeny} {
		reset(sc)
		onAuthFailure = func(_ context.Context, _ string) bool {
			if sc == scAuthRefuseDeny {
				setState("", false)
			}
			return false
		}
		wantCode := "permission_denied"
		if sc == scAuthRefuseDeny {
			wantCode = "unavailable"
		}
		_, err := cli.ListTransfers(context.Background(), owner, mb)
		var re *relayclient.Error
		if !errors.As(err, &re) || re.Code != wantCode || hits.Load() != 1 {
			t.Fatalf("control refuse sc=%d: err=%v hits=%d want=%s", sc, err, hits.Load(), wantCode)
		}
		reset(sc)
		err = cli.SubscribeEvents(context.Background(), owner, mb, "", func(relayclient.SSEEvent) error { return nil })
		if !errors.As(err, &re) || re.Code != wantCode || hits.Load() != 1 {
			t.Fatalf("SSE refuse sc=%d: err=%v hits=%d want=%s", sc, err, hits.Load(), wantCode)
		}
	}

	// (c) OnAuthFailure returns true, but admission is revoked BEFORE retry hits transport -> 0 retries, unavailable.
	reset(scRevokeBeforeRetrySend)
	onAuthFailure = func(_ context.Context, _ string) bool {
		setState("", false)
		return true
	}
	_, err = cli.ListTransfers(context.Background(), owner, mb)
	var re *relayclient.Error
	if !errors.As(err, &re) || re.Code != "unavailable" || hits.Load() != 1 {
		t.Fatalf("control revoke before retry: err=%v hits=%d", err, hits.Load())
	}
	reset(scRevokeBeforeRetrySend)
	err = cli.SubscribeEvents(context.Background(), owner, mb, "", func(relayclient.SSEEvent) error { return nil })
	if !errors.As(err, &re) || re.Code != "unavailable" || hits.Load() != 1 {
		t.Fatalf("SSE revoke before retry: err=%v hits=%d", err, hits.Load())
	}

	// (d) OnAuthFailure returns true, but provider yields unchanged/empty key -> 0 retries, permission_denied.
	for _, bogusKey := range []string{"k1", ""} {
		reset(scUnchangedKeyBeforeRetrySend)
		onAuthFailure = func(_ context.Context, _ string) bool {
			setState(bogusKey, true)
			return true
		}
		_, err = cli.ListTransfers(context.Background(), owner, mb)
		if !errors.As(err, &re) || re.Code != "permission_denied" || hits.Load() != 1 {
			t.Fatalf("control unchanged key %q: err=%v hits=%d", bogusKey, err, hits.Load())
		}
		reset(scUnchangedKeyBeforeRetrySend)
		err = cli.SubscribeEvents(context.Background(), owner, mb, "", func(relayclient.SSEEvent) error { return nil })
		if !errors.As(err, &re) || re.Code != "permission_denied" || hits.Load() != 1 {
			t.Fatalf("SSE unchanged key %q: err=%v hits=%d", bogusKey, err, hits.Load())
		}
	}

	// (e) Off-origin redirect blocked without hitting secondary host.
	reset(scRedirectOffOrigin)
	if _, err = cli.ListTransfers(context.Background(), owner, mb); !errors.As(err, &re) || re.Code != "origin_mismatch" || secondaryHits.Load() != 0 {
		t.Fatalf("expected origin_mismatch with 0 secondary hits, got err=%v secondary=%d", err, secondaryHits.Load())
	}

	// (f) Legitimate in-flight request completes after mid-flight revocation; if it returns 401, retry is blocked.
	reset(scInFlightSurvivesRevoke)
	entered1 := make(chan struct{})
	release1 := make(chan struct{})
	mu.Lock()
	inFlightEntered = entered1
	inFlightRelease = release1
	mu.Unlock()
	doneCh := make(chan error, 1)
	go func() {
		_, callErr := cli.ListTransfers(context.Background(), owner, mb)
		doneCh <- callErr
	}()
	<-entered1
	setState("", false)
	close(release1)
	if callErr := <-doneCh; callErr != nil || hits.Load() != 1 {
		t.Fatalf("expected legitimate in-flight 200 to complete once, err=%v hits=%d", callErr, hits.Load())
	}
	// Any subsequent request after revocation makes 0 transport hits and returns unavailable.
	hitsBefore := hits.Load()
	if _, callErr := cli.ListTransfers(context.Background(), owner, mb); !errors.As(callErr, &re) || re.Code != "unavailable" || hits.Load() != hitsBefore {
		t.Fatalf("expected post-revocation request blocked before transport, err=%v hits=%d/%d", callErr, hits.Load(), hitsBefore)
	}
	if sseErr := cli.SubscribeEvents(context.Background(), owner, mb, "", func(relayclient.SSEEvent) error { return nil }); !errors.As(sseErr, &re) || re.Code != "unavailable" || hits.Load() != hitsBefore {
		t.Fatalf("expected post-revocation SSE blocked before transport, err=%v hits=%d/%d", sseErr, hits.Load(), hitsBefore)
	}

	// In-flight request returning 401 while revoked blocks its retry at send boundary.
	reset(scInFlight401RetryRevoked)
	onAuthFailure = func(_ context.Context, _ string) bool { return true }
	entered2 := make(chan struct{})
	release2 := make(chan struct{})
	mu.Lock()
	inFlightEntered = entered2
	inFlightRelease = release2
	mu.Unlock()
	go func() {
		_, callErr := cli.ListTransfers(context.Background(), owner, mb)
		doneCh <- callErr
	}()
	<-entered2
	setState("", false)
	close(release2)
	if callErr := <-doneCh; !errors.As(callErr, &re) || re.Code != "unavailable" || hits.Load() != 1 {
		t.Fatalf("expected revoked 401 retry blocked with 0 retry hits, err=%v hits=%d", callErr, hits.Load())
	}
}
