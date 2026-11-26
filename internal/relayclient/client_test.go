package relayclient_test

import (
	"bytes"
	"context"
	"encoding/base64"
	"errors"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/relayclient"
	"github.com/uniclipboard/unic/internal/relaycrypto"
	"github.com/uniclipboard/unic/internal/relayserver"
)

func TestRelayClientServerRoundTripSingleAndMultiChunkAndAuth(t *testing.T) {
	sender, _ := identity.FromSeed(bytes.Repeat([]byte{0x11}, 32))
	recipient, _ := identity.FromSeed(bytes.Repeat([]byte{0x22}, 32))
	outsider, _ := identity.FromSeed(bytes.Repeat([]byte{0x33}, 32))

	accessKey := "self-host-secret-key"
	allowed := map[string]struct{}{
		base64.RawURLEncoding.EncodeToString(recipient.Public()): {},
	}
	srv := relayserver.New(relayserver.Config{
		AccessKey:      accessKey,
		AllowedPubKeys: allowed,
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
	})
	if err != nil {
		t.Fatal(err)
	}
	mb, err := relaycrypto.DeriveMailboxID(cli.Origin(), recipient.Public())
	if err != nil {
		t.Fatal(err)
	}

	// Wrong access key rejected.
	badKeyCli, _ := relayclient.New(relayclient.Config{BaseURL: ts.URL, AccessKey: "wrong", HTTPClient: ts.Client()})
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
	evtCh := make(chan relayclient.SSEEvent, 8)
	go func() {
		_ = cli.SubscribeEvents(sseCtx, recipient, mb, "", func(ev relayclient.SSEEvent) error {
			evtCh <- ev
			return nil
		})
	}()

	// Give SSE a brief moment to connect.
	time.Sleep(50 * time.Millisecond)

	// 1. Single-chunk 1-RTT upload + receive + ack.
	now := time.Now()
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
	if err := cli.UploadSingle(context.Background(), mb, token, env1, ct0); err != nil {
		t.Fatalf("UploadSingle: %v", err)
	}

	select {
	case ev := <-evtCh:
		if ev.Event != "transfer_single_ready" || ev.TransferID != tid1 {
			t.Fatalf("unexpected SSE event: %+v", ev)
		}
	case <-time.After(3 * time.Second):
		t.Fatalf("timed out waiting for SSE single event")
	}

	gotCT0, err := cli.DownloadChunk(context.Background(), recipient, mb, tid1, 0, 1<<20)
	if err != nil || !bytes.Equal(gotCT0, ct0) {
		t.Fatalf("DownloadChunk(0): err=%v", err)
	}
	if err := cli.AckTransfer(context.Background(), recipient, mb, tid1); err != nil {
		t.Fatalf("AckTransfer: %v", err)
	}
	if srv.ActiveTransferCount() != 0 {
		t.Fatalf("expected 0 active transfers after ack, got %d", srv.ActiveTransferCount())
	}

	// 2. Multi-chunk upload + list + download + delete.
	tid2 := "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
	cek2 := bytes.Repeat([]byte{0x66}, 32)
	p0 := bytes.Repeat([]byte{'A'}, relaycrypto.MinChunkSize)
	p1 := bytes.Repeat([]byte{'B'}, 1234)
	cc2, _ := relaycrypto.NewChunkCipher(cek2, tid2, sender.DeviceID(), recipient.DeviceID(), 2)
	c20, h20, _ := cc2.SealChunk(0, p0)
	c21, h21, _ := cc2.SealChunk(1, p1)
	env2, err := relaycrypto.SealEnvelope(sender, relaycrypto.SealEnvelopeInput{
		RelayOrigin:          cli.Origin(),
		TransferID:           tid2,
		RecipientPK:          recipient.Public(),
		CreatedMs:            now.UnixMilli(),
		ExpiresMs:            now.Add(relaycrypto.DefaultTTL).UnixMilli(),
		ChunkSize:            relaycrypto.MinChunkSize,
		TotalChunks:          2,
		TotalCiphertextBytes: int64(len(c20) + len(c21)),
		ChunkHashes:          [][32]byte{h20, h21},
		CEK:                  cek2,
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := cli.CreateTransfer(context.Background(), mb, token, env2); err != nil {
		t.Fatalf("CreateTransfer: %v", err)
	}
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 0, c20); err != nil {
		t.Fatalf("UploadChunk(0): %v", err)
	}
	if err := cli.UploadChunk(context.Background(), mb, token, tid2, 1, c21); err != nil {
		t.Fatalf("UploadChunk(1): %v", err)
	}
	list, err := cli.ListTransfers(context.Background(), recipient, mb)
	if err != nil || len(list) != 1 || list[0].TransferID != tid2 {
		t.Fatalf("ListTransfers: err=%v list=%+v", err, list)
	}
	if err := cli.DeleteTransferAsOwner(context.Background(), recipient, mb, tid2); err != nil {
		t.Fatalf("DeleteTransferAsOwner: %v", err)
	}
	if srv.ActiveTransferCount() != 0 {
		t.Fatalf("expected 0 active transfers after delete")
	}

	// 3. Revoke sender token and verify subsequent upload is forbidden.
	if err := cli.RevokeSender(context.Background(), recipient, mb, tokenHash); err != nil {
		t.Fatalf("RevokeSender: %v", err)
	}
	err = cli.UploadSingle(context.Background(), mb, token, env1, ct0)
	var re *relayclient.Error
	if !errors.As(err, &re) || re.Code != "permission_denied" {
		t.Fatalf("expected permission_denied after revoke, got %v", err)
	}

	// 4. SSRF origin check.
	if err := cli.ValidatePeerOrigin("https://127.0.0.1:9999"); err == nil {
		t.Fatalf("expected origin mismatch error")
	}
}
