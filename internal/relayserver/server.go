// Package relayserver implements the zero-knowledge Relay HTTP API v1 server
// (docs/contracts/hybrid-relay-contract.md §3). It stores only short-TTL
// ciphertext chunks and opaque mailbox/sender metadata, never plaintext or keys.
package relayserver

import (
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/relaycrypto"
)

const (
	DefaultMaxTransferBytes = 25 << 20 // 25 MiB
	DefaultMaxSenders       = 64
	DefaultMaxTransfers     = 32
	DefaultEventBuffer      = 128
	nonceReplayWindow       = 120 * time.Second
)

// Config configures a Relay HTTP API v1 server instance.
type Config struct {
	// Origin is the canonical origin used for Owner Auth and mailbox verification.
	// If empty, the server derives it from the incoming request (useful for httptest).
	Origin string
	// MaxTransferBytes bounds total ciphertext bytes per transfer (default 25 MiB).
	MaxTransferBytes int64
	// DefaultTTL and MaxTTL bound transfer lifetime (default 10m / 30m).
	DefaultTTL time.Duration
	MaxTTL     time.Duration
	// MaxSendersPerMailbox caps registered delivery token hashes per mailbox (default 64).
	MaxSendersPerMailbox int
	// MaxTransfersPerMailbox caps concurrent active transfers per mailbox (default 32).
	MaxTransfersPerMailbox int
	// AllowedPubKeys, when non-empty, restricts Owner Auth to listed base64url Ed25519 public keys.
	AllowedPubKeys map[string]struct{}
	// AccessKey, when non-empty, requires a matching X-Unic-Relay-Key header on every request.
	AccessKey string
	// KeepaliveInterval sets SSE comment heartbeat cadence (default 25s).
	KeepaliveInterval time.Duration
	// Now overrides time.Now for tests.
	Now func() time.Time
}

// TransferSummary is returned by GET /v1/relay/mailboxes/{mailbox_id}/transfers.
type TransferSummary struct {
	TransferID      string               `json:"transfer_id"`
	Envelope        relaycrypto.Envelope `json:"envelope"`
	AvailableBitmap string               `json:"available_bitmap"`
	ExpiresAtMs     int64                `json:"expires_at_ms"`
}

// SSEEvent is one server-sent event delivered on GET .../events.
type SSEEvent struct {
	ID              string                `json:"id"`
	Event           string                `json:"event"`
	TransferID      string                `json:"transfer_id"`
	Envelope        *relaycrypto.Envelope `json:"envelope,omitempty"`
	ChunkIndex      *int                  `json:"chunk_index,omitempty"`
	AvailableBitmap string                `json:"available_bitmap,omitempty"`
	ExpiresAtMs     int64                 `json:"expires_at_ms,omitempty"`
}

type senderRecord struct {
	tokenHash string
	maxBytes  int64
}

type storedTransfer struct {
	id         string
	senderHash string
	envelope   relaycrypto.Envelope
	hashes     [][32]byte
	chunks     [][]byte
	available  []byte
	expiresAt  time.Time
}

type sseSubscriber struct {
	ch   chan SSEEvent
	done chan struct{}
}

type mailboxState struct {
	senders   map[string]*senderRecord
	transfers map[string]*storedTransfer
	nextEvtID uint64
	history   []SSEEvent
	subs      map[*sseSubscriber]struct{}
}

// Server is an in-memory reference Relay HTTP API v1 server.
type Server struct {
	cfg Config

	mu        sync.Mutex
	mailboxes map[string]*mailboxState
	nonces    map[string]time.Time
	reqCount  int64
	closed    bool
}

// New creates a new Server with validated defaults.
func New(cfg Config) *Server {
	if cfg.MaxTransferBytes <= 0 {
		cfg.MaxTransferBytes = DefaultMaxTransferBytes
	}
	if cfg.DefaultTTL <= 0 {
		cfg.DefaultTTL = relaycrypto.DefaultTTL
	}
	if cfg.MaxTTL <= 0 {
		cfg.MaxTTL = relaycrypto.MaxTTL
	}
	if cfg.DefaultTTL > cfg.MaxTTL {
		cfg.DefaultTTL = cfg.MaxTTL
	}
	if cfg.MaxSendersPerMailbox <= 0 {
		cfg.MaxSendersPerMailbox = DefaultMaxSenders
	}
	if cfg.MaxTransfersPerMailbox <= 0 {
		cfg.MaxTransfersPerMailbox = DefaultMaxTransfers
	}
	if cfg.KeepaliveInterval <= 0 {
		cfg.KeepaliveInterval = 25 * time.Second
	}
	if cfg.Now == nil {
		cfg.Now = time.Now
	}
	if cfg.Origin != "" {
		if norm, err := relaycrypto.NormalizeRelayOrigin(cfg.Origin); err == nil {
			cfg.Origin = norm
		}
	}
	return &Server{
		cfg:       cfg,
		mailboxes: map[string]*mailboxState{},
		nonces:    map[string]time.Time{},
	}
}

// RequestCount returns the total number of HTTP requests handled (test observability).
func (s *Server) RequestCount() int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.reqCount
}

// ActiveTransferCount returns the number of currently stored non-expired transfers.
func (s *Server) ActiveTransferCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.cfg.Now()
	total := 0
	for _, mb := range s.mailboxes {
		s.sweepMailboxLocked(mb, now)
		total += len(mb.transfers)
	}
	return total
}

// Close closes active SSE subscribers.
func (s *Server) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return
	}
	s.closed = true
	for _, mb := range s.mailboxes {
		for sub := range mb.subs {
			close(sub.done)
			delete(mb.subs, sub)
		}
	}
}

// Sweep removes expired transfers and nonces.
func (s *Server) Sweep() {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.cfg.Now()
	for k, exp := range s.nonces {
		if !now.Before(exp) {
			delete(s.nonces, k)
		}
	}
	for id, mb := range s.mailboxes {
		s.sweepMailboxLocked(mb, now)
		if len(mb.senders) == 0 && len(mb.transfers) == 0 && len(mb.subs) == 0 {
			delete(s.mailboxes, id)
		}
	}
}

func (s *Server) sweepMailboxLocked(mb *mailboxState, now time.Time) {
	for tid, tr := range mb.transfers {
		if !now.Before(tr.expiresAt) {
			delete(mb.transfers, tid)
		}
	}
}

func (s *Server) originFor(r *http.Request) string {
	if s.cfg.Origin != "" {
		return s.cfg.Origin
	}
	scheme := "http"
	if r.TLS != nil || strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https") {
		scheme = "https"
	}
	norm, err := relaycrypto.NormalizeRelayOrigin(scheme + "://" + r.Host)
	if err != nil {
		return scheme + "://" + r.Host
	}
	return norm
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	s.mu.Lock()
	s.reqCount++
	s.mu.Unlock()

	if s.cfg.AccessKey != "" {
		got := r.Header.Get("X-Unic-Relay-Key")
		if subtle.ConstantTimeCompare([]byte(got), []byte(s.cfg.AccessKey)) != 1 {
			writeErr(w, http.StatusUnauthorized, "unauthorized")
			return
		}
	}

	const prefix = "/v1/relay/mailboxes/"
	if !strings.HasPrefix(r.URL.Path, prefix) {
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	rest := strings.TrimPrefix(r.URL.Path, prefix)
	parts := strings.Split(rest, "/")
	if len(parts) < 2 || !validMailboxID(parts[0]) {
		writeErr(w, http.StatusBadRequest, "invalid_path")
		return
	}
	mailboxID := parts[0]

	switch {
	case len(parts) == 3 && parts[1] == "senders":
		tokenHash := parts[2]
		switch r.Method {
		case http.MethodPut:
			s.handlePutSender(w, r, mailboxID, tokenHash)
		case http.MethodDelete:
			s.handleDeleteSender(w, r, mailboxID, tokenHash)
		default:
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
		}
	case len(parts) == 2 && parts[1] == "events" && r.Method == http.MethodGet:
		s.handleEvents(w, r, mailboxID)
	case len(parts) == 2 && parts[1] == "transfers":
		switch r.Method {
		case http.MethodGet:
			s.handleListTransfers(w, r, mailboxID)
		case http.MethodPost:
			s.handleCreateTransfer(w, r, mailboxID)
		default:
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
		}
	case len(parts) == 4 && parts[1] == "transfers" && parts[3] == "single" && r.Method == http.MethodPut:
		s.handleSingleTransfer(w, r, mailboxID, parts[2])
	case len(parts) == 5 && parts[1] == "transfers" && parts[3] == "chunks":
		idx, err := strconv.Atoi(parts[4])
		if err != nil || idx < 0 || idx >= relaycrypto.MaxChunks {
			writeErr(w, http.StatusBadRequest, "invalid_chunk_index")
			return
		}
		switch r.Method {
		case http.MethodPut:
			s.handleUploadChunk(w, r, mailboxID, parts[2], idx)
		case http.MethodGet:
			s.handleDownloadChunk(w, r, mailboxID, parts[2], idx)
		default:
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
		}
	case len(parts) == 4 && parts[1] == "transfers" && parts[3] == "ack" && r.Method == http.MethodPost:
		s.handleAckTransfer(w, r, mailboxID, parts[2])
	case len(parts) == 3 && parts[1] == "transfers" && r.Method == http.MethodDelete:
		s.handleDeleteTransfer(w, r, mailboxID, parts[2])
	default:
		writeErr(w, http.StatusNotFound, "not_found")
	}
}

func (s *Server) verifyOwner(r *http.Request, mailboxID string, body []byte) (int, error) {
	hdr := relaycrypto.OwnerAuthHeaders{
		PubKey:    r.Header.Get("X-Unic-PubKey"),
		Timestamp: r.Header.Get("X-Unic-Timestamp"),
		Nonce:     r.Header.Get("X-Unic-Nonce"),
		Sig:       r.Header.Get("X-Unic-Sig"),
	}
	if len(s.cfg.AllowedPubKeys) > 0 {
		if _, ok := s.cfg.AllowedPubKeys[hdr.PubKey]; !ok {
			return http.StatusForbidden, errors.New("pubkey not allowed")
		}
	}
	now := s.cfg.Now()
	if _, err := relaycrypto.VerifyOwnerAuth(s.originFor(r), mailboxID, r.Method, r.URL.RequestURI(), body, hdr, now); err != nil {
		return http.StatusUnauthorized, err
	}
	nonceKey := mailboxID + ":" + hdr.Nonce
	s.mu.Lock()
	defer s.mu.Unlock()
	for k, exp := range s.nonces {
		if !now.Before(exp) {
			delete(s.nonces, k)
		}
	}
	if _, dup := s.nonces[nonceKey]; dup {
		return http.StatusUnauthorized, errors.New("replayed nonce")
	}
	s.nonces[nonceKey] = now.Add(nonceReplayWindow)
	return http.StatusOK, nil
}

func bearerToken(r *http.Request) (string, bool) {
	auth := strings.TrimSpace(r.Header.Get("Authorization"))
	if !strings.HasPrefix(auth, "Bearer ") {
		return "", false
	}
	tok := strings.TrimSpace(strings.TrimPrefix(auth, "Bearer "))
	if len(tok) < 16 || len(tok) > 256 {
		return "", false
	}
	return tok, true
}

func (s *Server) mailboxLocked(id string, create bool) *mailboxState {
	mb := s.mailboxes[id]
	if mb == nil && create {
		mb = &mailboxState{
			senders:   map[string]*senderRecord{},
			transfers: map[string]*storedTransfer{},
			subs:      map[*sseSubscriber]struct{}{},
		}
		s.mailboxes[id] = mb
	}
	return mb
}

func (s *Server) authorizeSenderLocked(mailboxID, token string) (*mailboxState, *senderRecord, bool) {
	mb := s.mailboxes[mailboxID]
	if mb == nil {
		return nil, nil, false
	}
	th := relaycrypto.TokenHash(token)
	rec := mb.senders[th]
	if rec == nil {
		return nil, nil, false
	}
	return mb, rec, true
}

func (s *Server) publishLocked(mb *mailboxState, evt SSEEvent) {
	mb.nextEvtID++
	evt.ID = strconv.FormatUint(mb.nextEvtID, 10)
	mb.history = append(mb.history, evt)
	if len(mb.history) > DefaultEventBuffer {
		mb.history = mb.history[len(mb.history)-DefaultEventBuffer:]
	}
	for sub := range mb.subs {
		select {
		case sub.ch <- evt:
		default:
			// Slow consumer: drop oldest or let it resync via ListTransfers.
		}
	}
}

func (s *Server) handlePutSender(w http.ResponseWriter, r *http.Request, mailboxID, tokenHash string) {
	if !validHex64(tokenHash) {
		writeErr(w, http.StatusBadRequest, "invalid_token_hash")
		return
	}
	body, err := readLimited(r.Body, 4096)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "body_too_large")
		return
	}
	if code, err := s.verifyOwner(r, mailboxID, body); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	var req struct {
		MaxBytes int64 `json:"max_bytes,omitempty"`
	}
	if len(body) > 0 {
		if err := json.Unmarshal(body, &req); err != nil || req.MaxBytes < 0 {
			writeErr(w, http.StatusBadRequest, "invalid_json")
			return
		}
	}
	limit := s.cfg.MaxTransferBytes
	if req.MaxBytes > 0 && req.MaxBytes < limit {
		limit = req.MaxBytes
	}

	s.mu.Lock()
	mb := s.mailboxLocked(mailboxID, true)
	if _, exists := mb.senders[tokenHash]; !exists && len(mb.senders) >= s.cfg.MaxSendersPerMailbox {
		s.mu.Unlock()
		writeErr(w, http.StatusTooManyRequests, "too_many_senders")
		return
	}
	mb.senders[tokenHash] = &senderRecord{tokenHash: tokenHash, maxBytes: limit}
	s.mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) handleDeleteSender(w http.ResponseWriter, r *http.Request, mailboxID, tokenHash string) {
	if !validHex64(tokenHash) {
		writeErr(w, http.StatusBadRequest, "invalid_token_hash")
		return
	}
	body, err := readLimited(r.Body, 1024)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "body_too_large")
		return
	}
	if code, err := s.verifyOwner(r, mailboxID, body); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	s.mu.Lock()
	if mb := s.mailboxes[mailboxID]; mb != nil {
		delete(mb.senders, tokenHash)
	}
	s.mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) parseEnvelopeForStore(env relaycrypto.Envelope, expectedTransferID string, maxBytes int64, now time.Time) ([][32]byte, time.Time, int, string) {
	if env.V != relaycrypto.Version || env.TransferID == "" || (expectedTransferID != "" && env.TransferID != expectedTransferID) {
		return nil, time.Time{}, http.StatusBadRequest, "invalid_envelope"
	}
	if env.TotalChunks <= 0 || env.TotalChunks > relaycrypto.MaxChunks || len(env.ChunkHashes) != env.TotalChunks {
		return nil, time.Time{}, http.StatusBadRequest, "invalid_chunk_count"
	}
	if env.TotalCiphertextBytes <= 0 || env.TotalCiphertextBytes > maxBytes {
		return nil, time.Time{}, http.StatusRequestEntityTooLarge, "payload_too_large"
	}
	if env.TotalChunks == 1 {
		if env.ChunkSize <= 0 || env.ChunkSize > relaycrypto.SingleChunkThreshold {
			return nil, time.Time{}, http.StatusBadRequest, "invalid_chunk_size"
		}
	} else {
		if env.ChunkSize < relaycrypto.MinChunkSize || env.ChunkSize > relaycrypto.MaxChunkSize {
			return nil, time.Time{}, http.StatusBadRequest, "invalid_chunk_size"
		}
	}
	hashes := make([][32]byte, env.TotalChunks)
	for i, hs := range env.ChunkHashes {
		hb, err := base64.RawURLEncoding.DecodeString(hs)
		if err != nil || len(hb) != 32 {
			return nil, time.Time{}, http.StatusBadRequest, "invalid_chunk_hash"
		}
		copy(hashes[i][:], hb)
	}
	exp := now.Add(s.cfg.DefaultTTL)
	if env.ExpiresMs > 0 {
		envExp := time.UnixMilli(env.ExpiresMs)
		if !now.Before(envExp) {
			return nil, time.Time{}, http.StatusBadRequest, "expired"
		}
		if maxExp := now.Add(s.cfg.MaxTTL); envExp.After(maxExp) {
			exp = maxExp
		} else {
			exp = envExp
		}
	}
	return hashes, exp, http.StatusOK, ""
}

func (s *Server) handleSingleTransfer(w http.ResponseWriter, r *http.Request, mailboxID, transferID string) {
	tok, ok := bearerToken(r)
	if !ok {
		writeErr(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	s.mu.Lock()
	mb, sender, ok := s.authorizeSenderLocked(mailboxID, tok)
	if !ok {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	maxBytes := sender.maxBytes
	s.mu.Unlock()

	maxFrame := min(maxBytes, int64(relaycrypto.SingleChunkThreshold+relaycrypto.GCMTagSize)) + relaycrypto.MaxEnvelopeJSONBytes + 4
	frame, err := readLimited(r.Body, maxFrame)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "payload_too_large")
		return
	}
	env, ct0, err := relaycrypto.DecodeSingleFrame(frame)
	if err != nil || env.TotalChunks != 1 {
		writeErr(w, http.StatusBadRequest, "invalid_frame")
		return
	}
	now := s.cfg.Now()
	hashes, exp, status, reason := s.parseEnvelopeForStore(env, transferID, maxBytes, now)
	if status != http.StatusOK {
		writeErr(w, status, reason)
		return
	}
	if int64(len(ct0)) != env.TotalCiphertextBytes {
		writeErr(w, http.StatusBadRequest, "size_mismatch")
		return
	}
	sum := sha256.Sum256(ct0)
	if subtle.ConstantTimeCompare(sum[:], hashes[0][:]) != 1 {
		writeErr(w, http.StatusBadRequest, "chunk_hash_mismatch")
		return
	}
	bm, _ := relaycrypto.NewBitmap(1)
	relaycrypto.SetBit(bm, 0)

	s.mu.Lock()
	mb = s.mailboxes[mailboxID]
	if mb == nil || mb.senders[sender.tokenHash] == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	s.sweepMailboxLocked(mb, now)
	if _, dup := mb.transfers[transferID]; dup {
		s.mu.Unlock()
		writeErr(w, http.StatusConflict, "already_exists")
		return
	}
	if len(mb.transfers) >= s.cfg.MaxTransfersPerMailbox {
		s.mu.Unlock()
		writeErr(w, http.StatusTooManyRequests, "mailbox_full")
		return
	}
	mb.transfers[transferID] = &storedTransfer{
		id:         transferID,
		senderHash: sender.tokenHash,
		envelope:   env,
		hashes:     hashes,
		chunks:     [][]byte{ct0},
		available:  bm,
		expiresAt:  exp,
	}
	envCopy := env
	s.publishLocked(mb, SSEEvent{
		Event:           "transfer_single_ready",
		TransferID:      transferID,
		Envelope:        &envCopy,
		AvailableBitmap: relaycrypto.EncodeBitmap(bm),
		ExpiresAtMs:     exp.UnixMilli(),
	})
	s.mu.Unlock()

	writeJSON(w, http.StatusOK, map[string]any{
		"transfer_id":   transferID,
		"expires_at_ms": exp.UnixMilli(),
	})
}

func (s *Server) handleCreateTransfer(w http.ResponseWriter, r *http.Request, mailboxID string) {
	tok, ok := bearerToken(r)
	if !ok {
		writeErr(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	s.mu.Lock()
	_, sender, ok := s.authorizeSenderLocked(mailboxID, tok)
	if !ok {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	maxBytes := sender.maxBytes
	s.mu.Unlock()

	body, err := readLimited(r.Body, relaycrypto.MaxEnvelopeJSONBytes)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "envelope_too_large")
		return
	}
	var env relaycrypto.Envelope
	if err := json.Unmarshal(body, &env); err != nil {
		writeErr(w, http.StatusBadRequest, "invalid_json")
		return
	}
	now := s.cfg.Now()
	hashes, exp, status, reason := s.parseEnvelopeForStore(env, "", maxBytes, now)
	if status != http.StatusOK {
		writeErr(w, status, reason)
		return
	}
	bm, err := relaycrypto.NewBitmap(env.TotalChunks)
	if err != nil {
		writeErr(w, http.StatusBadRequest, "invalid_chunks")
		return
	}

	s.mu.Lock()
	mb := s.mailboxes[mailboxID]
	if mb == nil || mb.senders[sender.tokenHash] == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	s.sweepMailboxLocked(mb, now)
	if _, dup := mb.transfers[env.TransferID]; dup {
		s.mu.Unlock()
		writeErr(w, http.StatusConflict, "already_exists")
		return
	}
	if len(mb.transfers) >= s.cfg.MaxTransfersPerMailbox {
		s.mu.Unlock()
		writeErr(w, http.StatusTooManyRequests, "mailbox_full")
		return
	}
	mb.transfers[env.TransferID] = &storedTransfer{
		id:         env.TransferID,
		senderHash: sender.tokenHash,
		envelope:   env,
		hashes:     hashes,
		chunks:     make([][]byte, env.TotalChunks),
		available:  bm,
		expiresAt:  exp,
	}
	envCopy := env
	s.publishLocked(mb, SSEEvent{
		Event:           "transfer_announced",
		TransferID:      env.TransferID,
		Envelope:        &envCopy,
		AvailableBitmap: relaycrypto.EncodeBitmap(bm),
		ExpiresAtMs:     exp.UnixMilli(),
	})
	s.mu.Unlock()

	writeJSON(w, http.StatusOK, map[string]any{
		"transfer_id":     env.TransferID,
		"expires_at_ms":   exp.UnixMilli(),
		"max_chunk_bytes": relaycrypto.MaxChunkSize + relaycrypto.GCMTagSize,
	})
}

func (s *Server) handleUploadChunk(w http.ResponseWriter, r *http.Request, mailboxID, transferID string, idx int) {
	tok, ok := bearerToken(r)
	if !ok {
		writeErr(w, http.StatusUnauthorized, "unauthorized")
		return
	}
	now := s.cfg.Now()
	s.mu.Lock()
	mb, sender, ok := s.authorizeSenderLocked(mailboxID, tok)
	if !ok {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	s.sweepMailboxLocked(mb, now)
	tr := mb.transfers[transferID]
	if tr == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	if tr.senderHash != sender.tokenHash {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	if idx < 0 || idx >= tr.envelope.TotalChunks {
		s.mu.Unlock()
		writeErr(w, http.StatusBadRequest, "invalid_chunk_index")
		return
	}
	maxChunk := int64(tr.envelope.ChunkSize) + relaycrypto.GCMTagSize
	wantHash := tr.hashes[idx]
	s.mu.Unlock()

	body, err := readLimited(r.Body, maxChunk)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "chunk_too_large")
		return
	}
	sum := sha256.Sum256(body)
	if subtle.ConstantTimeCompare(sum[:], wantHash[:]) != 1 {
		writeErr(w, http.StatusBadRequest, "chunk_hash_mismatch")
		return
	}

	s.mu.Lock()
	mb = s.mailboxes[mailboxID]
	if mb == nil || mb.senders[sender.tokenHash] == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	s.sweepMailboxLocked(mb, s.cfg.Now())
	tr = mb.transfers[transferID]
	if tr == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusGone, "gone")
		return
	}
	tr.chunks[idx] = body
	relaycrypto.SetBit(tr.available, idx)
	ci := idx
	s.publishLocked(mb, SSEEvent{
		Event:           "chunk_ready",
		TransferID:      transferID,
		ChunkIndex:      &ci,
		AvailableBitmap: relaycrypto.EncodeBitmap(tr.available),
		ExpiresAtMs:     tr.expiresAt.UnixMilli(),
	})
	s.mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) handleListTransfers(w http.ResponseWriter, r *http.Request, mailboxID string) {
	if code, err := s.verifyOwner(r, mailboxID, nil); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	now := s.cfg.Now()
	s.mu.Lock()
	mb := s.mailboxes[mailboxID]
	out := []TransferSummary{}
	if mb != nil {
		s.sweepMailboxLocked(mb, now)
		for _, tr := range mb.transfers {
			out = append(out, TransferSummary{
				TransferID:      tr.id,
				Envelope:        tr.envelope,
				AvailableBitmap: relaycrypto.EncodeBitmap(tr.available),
				ExpiresAtMs:     tr.expiresAt.UnixMilli(),
			})
		}
	}
	s.mu.Unlock()
	writeJSON(w, http.StatusOK, map[string]any{"transfers": out})
}

func (s *Server) handleDownloadChunk(w http.ResponseWriter, r *http.Request, mailboxID, transferID string, idx int) {
	if code, err := s.verifyOwner(r, mailboxID, nil); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	now := s.cfg.Now()
	s.mu.Lock()
	mb := s.mailboxes[mailboxID]
	if mb == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	s.sweepMailboxLocked(mb, now)
	tr := mb.transfers[transferID]
	if tr == nil || idx >= len(tr.chunks) || tr.chunks[idx] == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	data := tr.chunks[idx]
	s.mu.Unlock()

	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Content-Length", strconv.Itoa(len(data)))
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}

func (s *Server) handleAckTransfer(w http.ResponseWriter, r *http.Request, mailboxID, transferID string) {
	body, err := readLimited(r.Body, 1024)
	if err != nil {
		writeErr(w, http.StatusRequestEntityTooLarge, "body_too_large")
		return
	}
	if code, err := s.verifyOwner(r, mailboxID, body); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	s.mu.Lock()
	if mb := s.mailboxes[mailboxID]; mb != nil {
		delete(mb.transfers, transferID)
	}
	s.mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) handleDeleteTransfer(w http.ResponseWriter, r *http.Request, mailboxID, transferID string) {
	if tok, ok := bearerToken(r); ok {
		s.mu.Lock()
		mb, sender, authOK := s.authorizeSenderLocked(mailboxID, tok)
		if !authOK {
			s.mu.Unlock()
			writeErr(w, http.StatusForbidden, "forbidden")
			return
		}
		if tr := mb.transfers[transferID]; tr != nil {
			if tr.senderHash != sender.tokenHash {
				s.mu.Unlock()
				writeErr(w, http.StatusForbidden, "forbidden")
				return
			}
			delete(mb.transfers, transferID)
		}
		s.mu.Unlock()
		w.WriteHeader(http.StatusNoContent)
		return
	}
	if code, err := s.verifyOwner(r, mailboxID, nil); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	s.mu.Lock()
	if mb := s.mailboxes[mailboxID]; mb != nil {
		delete(mb.transfers, transferID)
	}
	s.mu.Unlock()
	w.WriteHeader(http.StatusNoContent)
}

func (s *Server) handleEvents(w http.ResponseWriter, r *http.Request, mailboxID string) {
	if code, err := s.verifyOwner(r, mailboxID, nil); err != nil {
		writeErr(w, code, "unauthorized")
		return
	}
	flusher, ok := w.(http.Flusher)
	if !ok {
		writeErr(w, http.StatusInternalServerError, "streaming_unsupported")
		return
	}
	var afterID uint64
	if raw := strings.TrimSpace(r.Header.Get("Last-Event-ID")); raw != "" {
		if parsed, err := strconv.ParseUint(raw, 10, 64); err == nil {
			afterID = parsed
		}
	}

	sub := &sseSubscriber{ch: make(chan SSEEvent, 64), done: make(chan struct{})}
	var backlog []SSEEvent

	s.mu.Lock()
	if s.closed {
		s.mu.Unlock()
		writeErr(w, http.StatusServiceUnavailable, "closing")
		return
	}
	mb := s.mailboxLocked(mailboxID, true)
	if afterID > 0 {
		for _, ev := range mb.history {
			if id, err := strconv.ParseUint(ev.ID, 10, 64); err == nil && id > afterID {
				backlog = append(backlog, ev)
			}
		}
	}
	mb.subs[sub] = struct{}{}
	s.mu.Unlock()

	defer func() {
		s.mu.Lock()
		if mb := s.mailboxes[mailboxID]; mb != nil {
			delete(mb.subs, sub)
		}
		s.mu.Unlock()
	}()

	w.Header().Set("Content-Type", "text/event-stream")
	w.Header().Set("Cache-Control", "no-cache")
	w.Header().Set("Connection", "keep-alive")
	w.WriteHeader(http.StatusOK)
	_, _ = io.WriteString(w, ": connected\n\n")
	flusher.Flush()

	writeEvt := func(ev SSEEvent) bool {
		b, err := json.Marshal(ev)
		if err != nil {
			return true
		}
		if _, err := fmt.Fprintf(w, "id: %s\nevent: %s\ndata: %s\n\n", ev.ID, ev.Event, b); err != nil {
			return false
		}
		flusher.Flush()
		return true
	}

	for _, ev := range backlog {
		if !writeEvt(ev) {
			return
		}
	}

	ticker := time.NewTicker(s.cfg.KeepaliveInterval)
	defer ticker.Stop()
	for {
		select {
		case <-r.Context().Done():
			return
		case <-sub.done:
			return
		case ev := <-sub.ch:
			if !writeEvt(ev) {
				return
			}
		case <-ticker.C:
			if _, err := io.WriteString(w, ": keepalive\n\n"); err != nil {
				return
			}
			flusher.Flush()
		}
	}
}

func readLimited(r io.Reader, limit int64) ([]byte, error) {
	if r == nil {
		return nil, nil
	}
	lr := io.LimitReader(r, limit+1)
	b, err := io.ReadAll(lr)
	if err != nil {
		return nil, err
	}
	if int64(len(b)) > limit {
		return nil, errors.New("body exceeds limit")
	}
	return b, nil
}

func validMailboxID(id string) bool {
	if len(id) != 32 {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !((c >= 'a' && c <= 'z') || (c >= '2' && c <= '7')) {
			return false
		}
	}
	return true
}

func validHex64(s string) bool {
	if len(s) != 64 {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	b, _ := json.Marshal(v)
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_, _ = w.Write(b)
}

func writeErr(w http.ResponseWriter, status int, code string) {
	writeJSON(w, status, map[string]string{"error": code})
}
