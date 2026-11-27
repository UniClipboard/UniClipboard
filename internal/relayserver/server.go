// Package relayserver implements an in-memory Direct-to-S3 Hyper Relay API
// reference server (docs/contracts/hybrid-relay-contract.md §3) for local
// integration tests and development. It models the exact control-plane / S3
// data-plane split: issuing short-lived signed PUT/GET URLs, verifying
// credential stripping on storage requests, checking chunk existence and size
// on commit, and deleting stored objects on ACK, DELETE, or TTL expiry.
package relayserver

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
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
	DefaultPresignTTL       = 5 * time.Minute
	MaxInlineUploadURLs     = 16
	nonceReplayWindow       = 120 * time.Second
)

// Config configures a Direct-to-S3 reference relay server instance.
type Config struct {
	// Origin is the canonical origin used for Owner Auth and mailbox verification.
	// If empty, the server derives it from the incoming request (useful for httptest).
	Origin string
	// StorageBaseURL optionally points presigned URLs to a separate storage HTTP server.
	// When empty, presigned URLs point back to this server's /_s3/ handler.
	StorageBaseURL string
	// MaxTransferBytes bounds total ciphertext bytes per transfer (default 25 MiB).
	MaxTransferBytes int64
	// DefaultTTL and MaxTTL bound transfer lifetime (default 10m / 30m).
	DefaultTTL time.Duration
	MaxTTL     time.Duration
	// PresignTTL bounds presigned PUT/GET URL lifetime (default 5m).
	PresignTTL time.Duration
	// MaxInlineUploadURLs overrides how many presigned PUT URLs CreateTransfer
	// returns inline (default 16). Useful in tests to exercise upload-url refreshes.
	MaxInlineUploadURLs int
	// MaxSendersPerMailbox caps registered delivery token hashes per mailbox (default 64).
	MaxSendersPerMailbox int
	// MaxTransfersPerMailbox caps concurrent active transfers per mailbox (default 32).
	MaxTransfersPerMailbox int
	// AllowedPubKeys, when non-empty, restricts Owner Auth to listed base64url Ed25519 public keys.
	AllowedPubKeys map[string]struct{}
	// AccessKey, when non-empty, requires a matching X-Unic-Relay-Key header on every control-plane request.
	AccessKey string
	// KeepaliveInterval sets SSE comment heartbeat cadence (default 25s).
	KeepaliveInterval time.Duration
	// Now overrides time.Now for tests.
	Now func() time.Time
}

// ChunkUploadGrant is one presigned PUT grant returned on transfer creation.
type ChunkUploadGrant struct {
	Index       int               `json:"index"`
	Method      string            `json:"method"`
	URL         string            `json:"url"`
	Headers     map[string]string `json:"headers,omitempty"`
	ExpiresAtMs int64             `json:"expires_at_ms"`
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
	chunkSizes map[int]int64
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

// Server is an in-memory Direct-to-S3 Hyper Relay reference server.
type Server struct {
	cfg        Config
	signingKey [32]byte

	mu        sync.Mutex
	mailboxes map[string]*mailboxState
	objects   map[string][]byte // objectKey -> ciphertext bytes
	nonces    map[string]time.Time
	reqCount  int64
	closed    bool
}

// New creates a new Direct-to-S3 reference Server with validated defaults.
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
	if cfg.PresignTTL <= 0 {
		cfg.PresignTTL = DefaultPresignTTL
	}
	if cfg.MaxInlineUploadURLs <= 0 {
		cfg.MaxInlineUploadURLs = MaxInlineUploadURLs
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
	cfg.StorageBaseURL = strings.TrimRight(strings.TrimSpace(cfg.StorageBaseURL), "/")

	var sk [32]byte
	_, _ = rand.Read(sk[:])
	return &Server{
		cfg:        cfg,
		signingKey: sk,
		mailboxes:  map[string]*mailboxState{},
		objects:    map[string][]byte{},
		nonces:     map[string]time.Time{},
	}
}

// StorageHandler returns an http.Handler serving only the simulated S3 /_s3/
// presigned object endpoints, for tests that run S3 on a distinct origin.
func (s *Server) StorageHandler() http.Handler {
	return http.HandlerFunc(s.handleStorage)
}

// RequestCount returns the total number of control-plane HTTP requests handled.
func (s *Server) RequestCount() int64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.reqCount
}

// ActiveTransferCount returns the number of currently tracked non-expired transfers.
func (s *Server) ActiveTransferCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.cfg.Now()
	total := 0
	for mbID, mb := range s.mailboxes {
		s.sweepMailboxLocked(mbID, mb, now)
		total += len(mb.transfers)
	}
	return total
}

// StoredObjectCount returns the number of chunk objects currently stored in simulated S3.
func (s *Server) StoredObjectCount() int {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.cfg.Now()
	for mbID, mb := range s.mailboxes {
		s.sweepMailboxLocked(mbID, mb, now)
	}
	return len(s.objects)
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

// Sweep removes expired transfers, their S3 chunk objects, and expired nonces.
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
		s.sweepMailboxLocked(id, mb, now)
		if len(mb.senders) == 0 && len(mb.transfers) == 0 && len(mb.subs) == 0 {
			delete(s.mailboxes, id)
		}
	}
}

func objectKey(mailboxID, transferID string, idx int) string {
	return fmt.Sprintf("%s/%s/%d", mailboxID, transferID, idx)
}

func (s *Server) deleteTransferObjectsLocked(mailboxID string, tr *storedTransfer) {
	for i := 0; i < tr.envelope.TotalChunks; i++ {
		delete(s.objects, objectKey(mailboxID, tr.id, i))
	}
}

func (s *Server) sweepMailboxLocked(mailboxID string, mb *mailboxState, now time.Time) {
	for tid, tr := range mb.transfers {
		if !now.Before(tr.expiresAt) {
			s.deleteTransferObjectsLocked(mailboxID, tr)
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

func (s *Server) storageBaseFor(r *http.Request) string {
	if s.cfg.StorageBaseURL != "" {
		return s.cfg.StorageBaseURL
	}
	return s.originFor(r)
}

func (s *Server) presignTTLFor(now, transferExpires time.Time) time.Duration {
	remaining := transferExpires.Sub(now)
	if remaining <= 0 {
		return time.Second
	}
	if remaining < s.cfg.PresignTTL {
		return remaining
	}
	return s.cfg.PresignTTL
}

func (s *Server) signStorageToken(method, key string, expUnix int64) string {
	mac := hmac.New(sha256.New, s.signingKey[:])
	mac.Write([]byte(method))
	mac.Write([]byte{0})
	mac.Write([]byte(key))
	mac.Write([]byte{0})
	mac.Write([]byte(strconv.FormatInt(expUnix, 10)))
	return hex.EncodeToString(mac.Sum(nil))
}

func (s *Server) presignURL(r *http.Request, method, mailboxID, transferID string, idx int, ttl time.Duration, now time.Time) (string, int64) {
	if ttl < time.Second {
		ttl = time.Second
	}
	exp := now.Add(ttl)
	expUnix := exp.Unix()
	key := objectKey(mailboxID, transferID, idx)
	sig := s.signStorageToken(method, key, expUnix)
	q := url.Values{}
	q.Set("X-Amz-Expires", strconv.FormatInt(int64(ttl.Seconds()), 10))
	q.Set("exp", strconv.FormatInt(expUnix, 10))
	q.Set("X-Amz-Signature", sig)
	u := fmt.Sprintf("%s/_s3/%s?%s", s.storageBaseFor(r), key, q.Encode())
	return u, exp.UnixMilli()
}

func (s *Server) ServeHTTP(w http.ResponseWriter, r *http.Request) {
	if strings.HasPrefix(r.URL.Path, "/_s3/") {
		s.handleStorage(w, r)
		return
	}

	s.mu.Lock()
	s.reqCount++
	s.mu.Unlock()

	if r.URL.Path == "/healthz" {
		if r.Method != http.MethodGet && r.Method != http.MethodHead {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if r.Method == http.MethodHead {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			return
		}
		writeJSON(w, http.StatusOK, map[string]string{
			"status":  "ok",
			"service": "hyper-relay",
			"storage": "s3-direct",
		})
		return
	}

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
	case len(parts) == 2 && parts[1] == "events":
		if r.Method != http.MethodGet {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
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
	case len(parts) == 6 && parts[1] == "transfers" && parts[3] == "chunks" && parts[5] == "upload-url":
		if r.Method != http.MethodPost {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if !relaycrypto.ValidTransferID(parts[2]) {
			writeErr(w, http.StatusBadRequest, "invalid_transfer_id")
			return
		}
		idx, ok := parseChunkIndex(parts[4])
		if !ok {
			writeErr(w, http.StatusBadRequest, "invalid_chunk_index")
			return
		}
		s.handleChunkUploadURL(w, r, mailboxID, parts[2], idx)
	case len(parts) == 6 && parts[1] == "transfers" && parts[3] == "chunks" && parts[5] == "commit":
		if r.Method != http.MethodPost {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if !relaycrypto.ValidTransferID(parts[2]) {
			writeErr(w, http.StatusBadRequest, "invalid_transfer_id")
			return
		}
		idx, ok := parseChunkIndex(parts[4])
		if !ok {
			writeErr(w, http.StatusBadRequest, "invalid_chunk_index")
			return
		}
		s.handleCommitChunk(w, r, mailboxID, parts[2], idx)
	case len(parts) == 5 && parts[1] == "transfers" && parts[3] == "chunks":
		if r.Method != http.MethodGet {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if !relaycrypto.ValidTransferID(parts[2]) {
			writeErr(w, http.StatusBadRequest, "invalid_transfer_id")
			return
		}
		idx, ok := parseChunkIndex(parts[4])
		if !ok {
			writeErr(w, http.StatusBadRequest, "invalid_chunk_index")
			return
		}
		s.handleChunkDownloadURL(w, r, mailboxID, parts[2], idx)
	case len(parts) == 4 && parts[1] == "transfers" && parts[3] == "ack":
		if r.Method != http.MethodPost {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if !relaycrypto.ValidTransferID(parts[2]) {
			writeErr(w, http.StatusBadRequest, "invalid_transfer_id")
			return
		}
		s.handleAckTransfer(w, r, mailboxID, parts[2])
	case len(parts) == 3 && parts[1] == "transfers":
		if r.Method != http.MethodDelete {
			writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
			return
		}
		if !relaycrypto.ValidTransferID(parts[2]) {
			writeErr(w, http.StatusBadRequest, "invalid_transfer_id")
			return
		}
		s.handleDeleteTransfer(w, r, mailboxID, parts[2])
	default:
		writeErr(w, http.StatusNotFound, "not_found")
	}
}

func (s *Server) handleStorage(w http.ResponseWriter, r *http.Request) {
	// Enforce client security contract: presigned S3 requests MUST NOT carry relay credentials.
	for k := range r.Header {
		lk := strings.ToLower(k)
		if lk == "authorization" || lk == "x-unic-relay-key" || strings.HasPrefix(lk, "x-unic-") {
			writeErr(w, http.StatusBadRequest, "credential_leak")
			return
		}
	}
	if !strings.HasPrefix(r.URL.Path, "/_s3/") {
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	key := strings.TrimPrefix(r.URL.Path, "/_s3/")
	expRaw := r.URL.Query().Get("exp")
	sig := r.URL.Query().Get("X-Amz-Signature")
	expUnix, err := strconv.ParseInt(expRaw, 10, 64)
	if err != nil || sig == "" {
		writeErr(w, http.StatusForbidden, "invalid_signature")
		return
	}
	wantSig := s.signStorageToken(r.Method, key, expUnix)
	if subtle.ConstantTimeCompare([]byte(sig), []byte(wantSig)) != 1 {
		writeErr(w, http.StatusForbidden, "invalid_signature")
		return
	}
	if s.cfg.Now().Unix() > expUnix {
		writeErr(w, http.StatusForbidden, "signature_expired")
		return
	}

	switch r.Method {
	case http.MethodPut:
		// Allow slightly over MaxChunkSize so commit can test HeadObject 413 rejection.
		body, err := readLimited(r.Body, relaycrypto.MaxChunkSize+relaycrypto.GCMTagSize+(1<<20))
		if err != nil {
			writeErr(w, http.StatusRequestEntityTooLarge, "payload_too_large")
			return
		}
		s.mu.Lock()
		s.objects[key] = body
		s.mu.Unlock()
		w.WriteHeader(http.StatusOK)
	case http.MethodGet:
		s.mu.Lock()
		data, ok := s.objects[key]
		s.mu.Unlock()
		if !ok {
			writeErr(w, http.StatusNotFound, "not_found")
			return
		}
		w.Header().Set("Content-Type", "application/octet-stream")
		w.Header().Set("Content-Length", strconv.Itoa(len(data)))
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write(data)
	default:
		writeErr(w, http.StatusMethodNotAllowed, "method_not_allowed")
	}
}

func parseChunkIndex(raw string) (int, bool) {
	idx, err := strconv.Atoi(raw)
	if err != nil || idx < 0 || idx >= relaycrypto.MaxChunks {
		return 0, false
	}
	return idx, true
}

func ownerTimestampHeader(r *http.Request) string {
	if ts := r.Header.Get("X-Unic-Timestamp"); ts != "" {
		return ts
	}
	return r.Header.Get("X-Unic-TS")
}

func (s *Server) verifyOwner(r *http.Request, mailboxID string, body []byte) (int, error) {
	hdr := relaycrypto.OwnerAuthHeaders{
		PubKey:    r.Header.Get("X-Unic-PubKey"),
		Timestamp: ownerTimestampHeader(r),
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
			// Slow consumer: receiver can resync via ListTransfers.
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

func (s *Server) validateEnvelope(env relaycrypto.Envelope, maxBytes int64, now time.Time) (time.Time, int, string) {
	if env.V != relaycrypto.Version || !relaycrypto.ValidTransferID(env.TransferID) {
		return time.Time{}, http.StatusBadRequest, "invalid_envelope"
	}
	if env.TotalChunks <= 0 || env.TotalChunks > relaycrypto.MaxChunks || len(env.ChunkHashes) != env.TotalChunks {
		return time.Time{}, http.StatusBadRequest, "invalid_chunk_count"
	}
	if env.TotalCiphertextBytes <= 0 || env.TotalCiphertextBytes > maxBytes {
		return time.Time{}, http.StatusRequestEntityTooLarge, "payload_too_large"
	}
	if env.TotalChunks == 1 {
		if env.ChunkSize <= 0 || env.ChunkSize > relaycrypto.SingleChunkThreshold {
			return time.Time{}, http.StatusBadRequest, "invalid_chunk_size"
		}
	} else {
		if env.ChunkSize < relaycrypto.MinChunkSize || env.ChunkSize > relaycrypto.MaxChunkSize {
			return time.Time{}, http.StatusBadRequest, "invalid_chunk_size"
		}
	}
	for _, hs := range env.ChunkHashes {
		hb, err := base64.RawURLEncoding.DecodeString(hs)
		if err != nil || len(hb) != 32 {
			return time.Time{}, http.StatusBadRequest, "invalid_chunk_hash"
		}
	}
	exp := now.Add(s.cfg.DefaultTTL)
	if env.ExpiresMs > 0 {
		envExp := time.UnixMilli(env.ExpiresMs)
		if !now.Before(envExp) {
			return time.Time{}, http.StatusBadRequest, "expired"
		}
		if maxExp := now.Add(s.cfg.MaxTTL); envExp.After(maxExp) {
			exp = maxExp
		} else {
			exp = envExp
		}
	}
	return exp, http.StatusOK, ""
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
	senderHash := sender.tokenHash
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
	exp, status, reason := s.validateEnvelope(env, maxBytes, now)
	if status != http.StatusOK {
		writeErr(w, status, reason)
		return
	}
	bm, err := relaycrypto.NewBitmap(env.TotalChunks)
	if err != nil {
		writeErr(w, http.StatusBadRequest, "invalid_chunks")
		return
	}

	inlineCount := min(env.TotalChunks, s.cfg.MaxInlineUploadURLs)
	presignTTL := s.presignTTLFor(now, exp)
	grants := make([]ChunkUploadGrant, 0, inlineCount)
	for i := 0; i < inlineCount; i++ {
		u, expMs := s.presignURL(r, http.MethodPut, mailboxID, env.TransferID, i, presignTTL, now)
		grants = append(grants, ChunkUploadGrant{
			Index:       i,
			Method:      http.MethodPut,
			URL:         u,
			ExpiresAtMs: expMs,
		})
	}

	s.mu.Lock()
	mb := s.mailboxes[mailboxID]
	if mb == nil || mb.senders[senderHash] == nil {
		s.mu.Unlock()
		writeErr(w, http.StatusForbidden, "forbidden")
		return
	}
	s.sweepMailboxLocked(mailboxID, mb, now)
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
		senderHash: senderHash,
		envelope:   env,
		chunkSizes: map[int]int64{},
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
		"upload_mode":     "presigned",
		"upload_urls":     grants,
	})
}

func (s *Server) handleChunkUploadURL(w http.ResponseWriter, r *http.Request, mailboxID, transferID string, idx int) {
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
	s.sweepMailboxLocked(mailboxID, mb, now)
	tr := mb.transfers[transferID]
	if tr == nil {
		// Clean up any chunk object whose presigned PUT raced with a receiver
		// discard/ack/expiry so orphaned test-server objects cannot linger.
		delete(s.objects, objectKey(mailboxID, transferID, idx))
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
	expiresAt := tr.expiresAt
	s.mu.Unlock()

	u, expMs := s.presignURL(r, http.MethodPut, mailboxID, transferID, idx, s.presignTTLFor(now, expiresAt), now)
	writeJSON(w, http.StatusOK, map[string]any{
		"method":        http.MethodPut,
		"url":           u,
		"expires_at_ms": expMs,
	})
}

func (s *Server) handleCommitChunk(w http.ResponseWriter, r *http.Request, mailboxID, transferID string, idx int) {
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
	s.sweepMailboxLocked(mailboxID, mb, now)
	tr := mb.transfers[transferID]
	if tr == nil {
		// Clean up any chunk object whose presigned PUT raced with a receiver
		// discard/ack/expiry so orphaned test-server objects cannot linger.
		delete(s.objects, objectKey(mailboxID, transferID, idx))
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
	key := objectKey(mailboxID, transferID, idx)
	obj, exists := s.objects[key]
	if !exists {
		s.mu.Unlock()
		writeErr(w, http.StatusNotFound, "chunk_not_uploaded")
		return
	}
	maxChunk := int64(tr.envelope.ChunkSize) + relaycrypto.GCMTagSize
	size := int64(len(obj))
	if size <= 0 || size > maxChunk || (tr.envelope.TotalChunks == 1 && size != tr.envelope.TotalCiphertextBytes) {
		delete(s.objects, key)
		s.mu.Unlock()
		if size > maxChunk {
			writeErr(w, http.StatusRequestEntityTooLarge, "chunk_too_large")
		} else {
			writeErr(w, http.StatusBadRequest, "size_mismatch")
		}
		return
	}
	tr.chunkSizes[idx] = size
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
		s.sweepMailboxLocked(mailboxID, mb, now)
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

func (s *Server) handleChunkDownloadURL(w http.ResponseWriter, r *http.Request, mailboxID, transferID string, idx int) {
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
	s.sweepMailboxLocked(mailboxID, mb, now)
	tr := mb.transfers[transferID]
	if tr == nil || idx < 0 || idx >= tr.envelope.TotalChunks || !relaycrypto.HasBit(tr.available, idx) {
		s.mu.Unlock()
		writeErr(w, http.StatusNotFound, "not_found")
		return
	}
	size := tr.chunkSizes[idx]
	expiresAt := tr.expiresAt
	s.mu.Unlock()

	u, expMs := s.presignURL(r, http.MethodGet, mailboxID, transferID, idx, s.presignTTLFor(now, expiresAt), now)
	writeJSON(w, http.StatusOK, map[string]any{
		"method":        http.MethodGet,
		"url":           u,
		"expires_at_ms": expMs,
		"size":          size,
	})
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
		if tr := mb.transfers[transferID]; tr != nil {
			s.deleteTransferObjectsLocked(mailboxID, tr)
			delete(mb.transfers, transferID)
		}
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
			s.deleteTransferObjectsLocked(mailboxID, tr)
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
		if tr := mb.transfers[transferID]; tr != nil {
			s.deleteTransferObjectsLocked(mailboxID, tr)
			delete(mb.transfers, transferID)
		}
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
		if (c < 'a' || c > 'z') && (c < '2' || c > '7') {
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
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') {
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
