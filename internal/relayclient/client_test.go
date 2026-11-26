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
	for !(sawAnnounced && sawReady) {
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
