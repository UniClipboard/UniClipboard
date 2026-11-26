// Package relaycrypto implements the zero-knowledge E2EE envelope, chunk AEAD,
// mailbox derivation, Owner Auth signing and bitmap encoding of the hybrid
// relay contract (docs/contracts/hybrid-relay-contract.md).
package relaycrypto

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/ecdh"
	"crypto/ed25519"
	"crypto/hkdf"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base32"
	"encoding/base64"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"strconv"
	"strings"
	"time"

	"filippo.io/edwards25519"

	"github.com/uniclipboard/unic/internal/identity"
)

const (
	Version = 1

	SingleChunkThreshold = 2 << 20   // 2 MiB: 1-RTT single-chunk fast path
	DefaultChunkSize     = 1 << 20   // 1 MiB plaintext chunk size
	MinChunkSize         = 256 << 10 // 256 KiB
	MaxChunkSize         = 4 << 20   // 4 MiB
	MaxChunks            = 4096
	MaxEnvelopeJSONBytes = 256 << 10 // 256 KiB hard cap for outer envelope JSON

	DefaultTTL       = 10 * time.Minute
	MaxTTL           = 30 * time.Minute
	MaxClockSkew     = 5 * time.Minute
	OwnerAuthMaxSkew = 60 * time.Second

	GCMTagSize   = 16
	GCMNonceSize = 12
	CEKSize      = 32
	WrappedSize  = CEKSize + GCMTagSize // 48 bytes
)

var (
	ErrInvalidInput    = errors.New("relaycrypto: invalid input")
	ErrBadKey          = errors.New("relaycrypto: invalid public or private key")
	ErrBadSignature    = errors.New("relaycrypto: invalid signature")
	ErrBadEnvelope     = errors.New("relaycrypto: invalid envelope")
	ErrExpired         = errors.New("relaycrypto: envelope expired or invalid timestamps")
	ErrDecryptFailed   = errors.New("relaycrypto: decryption failed")
	ErrChunkHash       = errors.New("relaycrypto: chunk hash mismatch")
	ErrInvalidOrigin   = errors.New("relaycrypto: invalid relay origin")
	ErrInvalidTransfer = errors.New("relaycrypto: invalid transfer id")
)

var b64 = base64.RawURLEncoding

// Signer abstracts the device identity operations kept inside Engine.
type Signer interface {
	DeviceID() string
	Public() []byte
	RelayX25519Key() (*ecdh.PrivateKey, error)
	SignRelayEnvelope(bodyAndWrappedCEK []byte) []byte
	SignRelayAuth(authPayload []byte) []byte
}

// Envelope is the outer signed metadata visible to the relay server.
// It contains no clipboard content, filenames, MIME types or source app names.
type Envelope struct {
	V                    int      `json:"v"`
	TransferID           string   `json:"transfer_id"`
	SenderID             string   `json:"sender_id"`
	RecipientID          string   `json:"recipient_id"`
	CreatedMs            int64    `json:"created_ms"`
	ExpiresMs            int64    `json:"expires_ms"`
	ChunkSize            int      `json:"chunk_size"`
	TotalChunks          int      `json:"total_chunks"`
	TotalCiphertextBytes int64    `json:"total_ciphertext_bytes"`
	EphPub               string   `json:"eph_pub"`
	WrappedCEK           string   `json:"wrapped_cek"`
	ChunkHashes          []string `json:"chunk_hashes"`
	Sig                  string   `json:"sig"`
}

func field(b []byte) []byte {
	out := make([]byte, 2, 2+len(b))
	binary.BigEndian.PutUint16(out, uint16(len(b)))
	return append(out, b...)
}

func cat(parts ...[]byte) []byte {
	n := 0
	for _, p := range parts {
		n += len(p)
	}
	out := make([]byte, 0, n)
	for _, p := range parts {
		out = append(out, p...)
	}
	return out
}

// NormalizeRelayOrigin canonicalizes a relay base URL into a stable origin string
// (lowercase scheme/host, default port stripped, no trailing slash, no query/fragment/userinfo).
func NormalizeRelayOrigin(raw string) (string, error) {
	s := strings.TrimSpace(raw)
	if s == "" {
		return "", ErrInvalidOrigin
	}
	u, err := url.Parse(s)
	if err != nil {
		return "", ErrInvalidOrigin
	}
	scheme := strings.ToLower(u.Scheme)
	if scheme != "https" && scheme != "http" {
		return "", ErrInvalidOrigin
	}
	if u.Host == "" || u.User != nil || u.RawQuery != "" || u.Fragment != "" || u.Opaque != "" {
		return "", ErrInvalidOrigin
	}
	host := strings.ToLower(u.Hostname())
	if host == "" {
		return "", ErrInvalidOrigin
	}
	port := u.Port()
	if (scheme == "https" && port == "443") || (scheme == "http" && port == "80") {
		port = ""
	}
	hostPort := host
	if strings.Contains(host, ":") {
		hostPort = "[" + host + "]"
	}
	if port != "" {
		hostPort += ":" + port
	}
	path := strings.TrimRight(u.EscapedPath(), "/")
	return scheme + "://" + hostPort + path, nil
}

// DeriveMailboxID derives the opaque 32-char lowercase base32 mailbox ID for
// recipientPK on relayOrigin.
func DeriveMailboxID(relayOrigin string, recipientPK []byte) (string, error) {
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return "", err
	}
	if len(recipientPK) != ed25519.PublicKeySize {
		return "", ErrBadKey
	}
	sum := sha256.Sum256(cat([]byte("unic/relay-mailbox/v1\x00"), field([]byte(origin)), recipientPK))
	return strings.ToLower(base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(sum[:20])), nil
}

// TokenHash returns the lowercase hex SHA-256 digest of a delivery token.
func TokenHash(deliveryToken string) string {
	sum := sha256.Sum256([]byte(deliveryToken))
	return fmt.Sprintf("%x", sum[:])
}

// ValidTransferID reports whether id is a valid 16..64 lowercase hex transfer identifier.
func ValidTransferID(id string) bool {
	if len(id) < 16 || len(id) > 64 {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

// NewDeliveryToken generates a fresh 256-bit base64url delivery token.
func NewDeliveryToken() (string, error) {
	var b [32]byte
	if _, err := rand.Read(b[:]); err != nil {
		return "", err
	}
	return b64.EncodeToString(b[:]), nil
}

// Ed25519PublicToX25519 converts a canonical non-small-order Ed25519 public key
// into its Montgomery X25519 public key.
func Ed25519PublicToX25519(pk []byte) (*ecdh.PublicKey, error) {
	if len(pk) != ed25519.PublicKeySize {
		return nil, ErrBadKey
	}
	pt, err := new(edwards25519.Point).SetBytes(pk)
	if err != nil {
		return nil, ErrBadKey
	}
	// Reject small-order points (cofactor * P == identity).
	cofactorCheck := new(edwards25519.Point).MultByCofactor(pt)
	if cofactorCheck.Equal(edwards25519.NewIdentityPoint()) == 1 {
		return nil, ErrBadKey
	}
	return ecdh.X25519().NewPublicKey(pt.BytesMontgomery())
}

// ChunkParams decides the chunk size and count for a plaintext byte length.
func ChunkParams(plaintextLen int64) (chunkSize int, totalChunks int, err error) {
	if plaintextLen <= 0 {
		return 0, 0, ErrInvalidInput
	}
	if plaintextLen <= SingleChunkThreshold {
		return int(plaintextLen), 1, nil
	}
	chunkSize = DefaultChunkSize
	total := (plaintextLen + int64(chunkSize) - 1) / int64(chunkSize)
	if total <= 0 || total > MaxChunks {
		return 0, 0, ErrInvalidInput
	}
	return chunkSize, int(total), nil
}

func validTransferID(id string) bool {
	if len(id) < 16 || len(id) > 64 {
		return false
	}
	for i := 0; i < len(id); i++ {
		c := id[i]
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

// ChunkCipher derives chunk encryption/decryption state from a CEK and transfer ID.
type ChunkCipher struct {
	aead        cipher.AEAD
	noncePrefix [8]byte
	transferID  string
	senderID    string
	recipientID string
	totalChunks int
}

// NewChunkCipher builds a per-transfer chunk AEAD cipher.
func NewChunkCipher(cek []byte, transferID, senderID, recipientID string, totalChunks int) (*ChunkCipher, error) {
	if len(cek) != CEKSize || !validTransferID(transferID) || senderID == "" || recipientID == "" || totalChunks <= 0 || totalChunks > MaxChunks {
		return nil, ErrInvalidInput
	}
	salt := sha256.Sum256(cat([]byte("unic/relay-chunk-salt/v1\x00"), field([]byte(transferID))))
	chunkKey, err := hkdf.Key(sha256.New, cek, salt[:], "unic/relay-chunk-key/v1\x00", 32)
	if err != nil {
		return nil, err
	}
	noncePrefixBytes, err := hkdf.Key(sha256.New, cek, salt[:], "unic/relay-chunk-nonce/v1\x00", 8)
	if err != nil {
		return nil, err
	}
	block, err := aes.NewCipher(chunkKey)
	if err != nil {
		return nil, err
	}
	aead, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	cc := &ChunkCipher{
		aead:        aead,
		transferID:  transferID,
		senderID:    senderID,
		recipientID: recipientID,
		totalChunks: totalChunks,
	}
	copy(cc.noncePrefix[:], noncePrefixBytes)
	return cc, nil
}

func (c *ChunkCipher) nonceAndAAD(index int) ([]byte, []byte, error) {
	if index < 0 || index >= c.totalChunks {
		return nil, nil, ErrInvalidInput
	}
	nonce := make([]byte, GCMNonceSize)
	copy(nonce[:8], c.noncePrefix[:])
	binary.BigEndian.PutUint32(nonce[8:], uint32(index))
	var idxBuf [9]byte
	binary.BigEndian.PutUint32(idxBuf[0:4], uint32(index))
	binary.BigEndian.PutUint32(idxBuf[4:8], uint32(c.totalChunks))
	if index == c.totalChunks-1 {
		idxBuf[8] = 1
	}
	aad := cat(
		[]byte("unic/relay-chunk-aad/v1\x00"),
		field([]byte(c.transferID)),
		field([]byte(c.senderID)),
		field([]byte(c.recipientID)),
		idxBuf[:],
	)
	return nonce, aad, nil
}

// SealChunk encrypts plaintext chunk index and returns (ciphertext, sha256(ciphertext)).
func (c *ChunkCipher) SealChunk(index int, plaintext []byte) ([]byte, [32]byte, error) {
	nonce, aad, err := c.nonceAndAAD(index)
	if err != nil {
		return nil, [32]byte{}, err
	}
	ct := c.aead.Seal(nil, nonce, plaintext, aad)
	return ct, sha256.Sum256(ct), nil
}

// OpenChunk verifies expectedHash against ciphertext and decrypts chunk index.
func (c *ChunkCipher) OpenChunk(index int, ciphertext []byte, expectedHash [32]byte) ([]byte, error) {
	if len(ciphertext) < GCMTagSize {
		return nil, ErrDecryptFailed
	}
	sum := sha256.Sum256(ciphertext)
	if subtle.ConstantTimeCompare(sum[:], expectedHash[:]) != 1 {
		return nil, ErrChunkHash
	}
	nonce, aad, err := c.nonceAndAAD(index)
	if err != nil {
		return nil, err
	}
	pt, err := c.aead.Open(nil, nonce, ciphertext, aad)
	if err != nil {
		return nil, ErrDecryptFailed
	}
	return pt, nil
}

// CanonicalEnvelopeBody computes the exact binary representation covered by envelope AAD and signature.
func CanonicalEnvelopeBody(relayOrigin string, env Envelope, rawEphPub []byte, rawChunkHashes [][32]byte) ([]byte, error) {
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return nil, err
	}
	if env.V != Version || !validTransferID(env.TransferID) || env.SenderID == "" || env.RecipientID == "" ||
		env.CreatedMs <= 0 || env.ExpiresMs <= env.CreatedMs ||
		env.ChunkSize <= 0 || env.TotalChunks <= 0 || env.TotalChunks > MaxChunks ||
		env.TotalCiphertextBytes <= 0 || len(rawEphPub) != 32 || len(rawChunkHashes) != env.TotalChunks {
		return nil, ErrBadEnvelope
	}
	h := sha256.New()
	for i := range rawChunkHashes {
		_, _ = h.Write(rawChunkHashes[i][:])
	}
	hashesRoot := h.Sum(nil)

	var nums [32]byte
	binary.BigEndian.PutUint64(nums[0:8], uint64(env.CreatedMs))
	binary.BigEndian.PutUint64(nums[8:16], uint64(env.ExpiresMs))
	binary.BigEndian.PutUint32(nums[16:20], uint32(env.ChunkSize))
	binary.BigEndian.PutUint32(nums[20:24], uint32(env.TotalChunks))
	binary.BigEndian.PutUint64(nums[24:32], uint64(env.TotalCiphertextBytes))

	return cat(
		[]byte("unic/relay-envelope/v1\x00"),
		field([]byte(origin)),
		[]byte{byte(env.V)},
		field([]byte(env.TransferID)),
		field([]byte(env.SenderID)),
		field([]byte(env.RecipientID)),
		nums[:],
		rawEphPub,
		hashesRoot,
	), nil
}

func envelopeAAD(body []byte) [32]byte {
	return sha256.Sum256(cat([]byte("unic/relay-envelope-aad/v1\x00"), body))
}

func deriveWrapParams(relayOrigin, senderID, recipientID, transferID string, shared, ephPub, recipientX25519 []byte) (key []byte, nonce []byte, err error) {
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return nil, nil, err
	}
	salt := sha256.Sum256(cat(
		[]byte("unic/relay-wrap-salt/v1\x00"),
		field([]byte(origin)),
		field([]byte(senderID)),
		field([]byte(recipientID)),
		field([]byte(transferID)),
	))
	keyInfo := string(cat([]byte("unic/relay-wrap-key/v1\x00"), ephPub, recipientX25519))
	nonceInfo := string(cat([]byte("unic/relay-wrap-nonce/v1\x00"), ephPub, recipientX25519))
	key, err = hkdf.Key(sha256.New, shared, salt[:], keyInfo, 32)
	if err != nil {
		return nil, nil, err
	}
	nonce, err = hkdf.Key(sha256.New, shared, salt[:], nonceInfo, GCMNonceSize)
	if err != nil {
		return nil, nil, err
	}
	return key, nonce, nil
}

// SealEnvelopeInput holds parameters for sealing an outer envelope.
type SealEnvelopeInput struct {
	RelayOrigin          string
	TransferID           string
	RecipientPK          []byte
	CreatedMs            int64
	ExpiresMs            int64
	ChunkSize            int
	TotalChunks          int
	TotalCiphertextBytes int64
	ChunkHashes          [][32]byte
	CEK                  []byte
	// Optional deterministic override for test vectors only; nil uses CSPRNG.
	EphemeralPriv *ecdh.PrivateKey
}

// SealEnvelope wraps CEK for recipientPK and signs the canonical envelope with sender.
func SealEnvelope(sender Signer, in SealEnvelopeInput) (Envelope, error) {
	if sender == nil || len(in.CEK) != CEKSize {
		return Envelope{}, ErrInvalidInput
	}
	origin, err := NormalizeRelayOrigin(in.RelayOrigin)
	if err != nil {
		return Envelope{}, err
	}
	recipientX, err := Ed25519PublicToX25519(in.RecipientPK)
	if err != nil {
		return Envelope{}, err
	}
	ephPriv := in.EphemeralPriv
	if ephPriv == nil {
		ephPriv, err = ecdh.X25519().GenerateKey(rand.Reader)
		if err != nil {
			return Envelope{}, err
		}
	}
	shared, err := ephPriv.ECDH(recipientX)
	if err != nil {
		return Envelope{}, ErrBadKey
	}
	ephPub := ephPriv.PublicKey().Bytes()
	senderID := sender.DeviceID()
	recipientID := identity.DeviceID(in.RecipientPK)

	env := Envelope{
		V:                    Version,
		TransferID:           in.TransferID,
		SenderID:             senderID,
		RecipientID:          recipientID,
		CreatedMs:            in.CreatedMs,
		ExpiresMs:            in.ExpiresMs,
		ChunkSize:            in.ChunkSize,
		TotalChunks:          in.TotalChunks,
		TotalCiphertextBytes: in.TotalCiphertextBytes,
		EphPub:               b64.EncodeToString(ephPub),
		ChunkHashes:          make([]string, len(in.ChunkHashes)),
	}
	for i, h := range in.ChunkHashes {
		env.ChunkHashes[i] = b64.EncodeToString(h[:])
	}
	body, err := CanonicalEnvelopeBody(origin, env, ephPub, in.ChunkHashes)
	if err != nil {
		return Envelope{}, err
	}
	aad := envelopeAAD(body)
	wrapKey, wrapNonce, err := deriveWrapParams(origin, senderID, recipientID, in.TransferID, shared, ephPub, recipientX.Bytes())
	if err != nil {
		return Envelope{}, err
	}
	block, err := aes.NewCipher(wrapKey)
	if err != nil {
		return Envelope{}, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return Envelope{}, err
	}
	wrapped := gcm.Seal(nil, wrapNonce, in.CEK, aad[:])
	env.WrappedCEK = b64.EncodeToString(wrapped)
	sig := sender.SignRelayEnvelope(cat(body, wrapped))
	env.Sig = b64.EncodeToString(sig)
	return env, nil
}

// VerifiedEnvelope holds parsed binary fields of a verified envelope.
type VerifiedEnvelope struct {
	Envelope    Envelope
	EphPub      []byte
	WrappedCEK  []byte
	ChunkHashes [][32]byte
	Body        []byte
}

// VerifyEnvelope performs mandatory structural, timestamp, identity and signature
// checks per hybrid-relay-contract §2.5 before any key unwrap or disk write.
func VerifyEnvelope(relayOrigin string, env Envelope, selfID string, senderPK []byte, now time.Time) (VerifiedEnvelope, error) {
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return VerifiedEnvelope{}, err
	}
	if env.V != Version || env.RecipientID != selfID || env.SenderID == "" || env.SenderID == selfID {
		return VerifiedEnvelope{}, ErrBadEnvelope
	}
	if err := identity.CheckID(env.SenderID, senderPK); err != nil {
		return VerifiedEnvelope{}, ErrBadKey
	}
	nowMs := now.UnixMilli()
	if env.CreatedMs <= 0 || env.ExpiresMs <= nowMs ||
		env.CreatedMs > nowMs+MaxClockSkew.Milliseconds() ||
		env.ExpiresMs-env.CreatedMs > MaxTTL.Milliseconds() {
		return VerifiedEnvelope{}, ErrExpired
	}
	if env.TotalChunks <= 0 || env.TotalChunks > MaxChunks || len(env.ChunkHashes) != env.TotalChunks {
		return VerifiedEnvelope{}, ErrBadEnvelope
	}
	if env.TotalChunks == 1 {
		if env.ChunkSize <= 0 || env.ChunkSize > SingleChunkThreshold {
			return VerifiedEnvelope{}, ErrBadEnvelope
		}
	} else {
		if env.ChunkSize < MinChunkSize || env.ChunkSize > MaxChunkSize {
			return VerifiedEnvelope{}, ErrBadEnvelope
		}
	}
	ephPub, err := b64.DecodeString(env.EphPub)
	if err != nil || len(ephPub) != 32 {
		return VerifiedEnvelope{}, ErrBadEnvelope
	}
	wrapped, err := b64.DecodeString(env.WrappedCEK)
	if err != nil || len(wrapped) != WrappedSize {
		return VerifiedEnvelope{}, ErrBadEnvelope
	}
	sig, err := b64.DecodeString(env.Sig)
	if err != nil || len(sig) != ed25519.SignatureSize {
		return VerifiedEnvelope{}, ErrBadSignature
	}
	hashes := make([][32]byte, env.TotalChunks)
	for i, hs := range env.ChunkHashes {
		hb, herr := b64.DecodeString(hs)
		if herr != nil || len(hb) != 32 {
			return VerifiedEnvelope{}, ErrBadEnvelope
		}
		copy(hashes[i][:], hb)
	}
	body, err := CanonicalEnvelopeBody(origin, env, ephPub, hashes)
	if err != nil {
		return VerifiedEnvelope{}, err
	}
	signedMsg := cat([]byte("unic/relay-envelope-sig/v1\x00"), body, wrapped)
	if !ed25519.Verify(senderPK, signedMsg, sig) {
		return VerifiedEnvelope{}, ErrBadSignature
	}
	return VerifiedEnvelope{
		Envelope:    env,
		EphPub:      ephPub,
		WrappedCEK:  wrapped,
		ChunkHashes: hashes,
		Body:        body,
	}, nil
}

// OpenCEK unwraps the content encryption key from a verified envelope using recipient's identity.
func OpenCEK(recipient Signer, relayOrigin string, ve VerifiedEnvelope) ([]byte, error) {
	if recipient == nil {
		return nil, ErrInvalidInput
	}
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return nil, err
	}
	priv, err := recipient.RelayX25519Key()
	if err != nil {
		return nil, ErrBadKey
	}
	ephPub, err := ecdh.X25519().NewPublicKey(ve.EphPub)
	if err != nil {
		return nil, ErrBadKey
	}
	shared, err := priv.ECDH(ephPub)
	if err != nil {
		return nil, ErrDecryptFailed
	}
	wrapKey, wrapNonce, err := deriveWrapParams(origin, ve.Envelope.SenderID, ve.Envelope.RecipientID, ve.Envelope.TransferID, shared, ve.EphPub, priv.PublicKey().Bytes())
	if err != nil {
		return nil, err
	}
	block, err := aes.NewCipher(wrapKey)
	if err != nil {
		return nil, err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, err
	}
	aad := envelopeAAD(ve.Body)
	cek, err := gcm.Open(nil, wrapNonce, ve.WrappedCEK, aad[:])
	if err != nil || len(cek) != CEKSize {
		return nil, ErrDecryptFailed
	}
	return cek, nil
}

// ---- Owner Auth (HTTP request signing) ----

// OwnerAuthPayload builds the canonical bytes signed for Owner Auth headers.
func OwnerAuthPayload(relayOrigin, method, pathAndQuery, timestamp, nonce string, bodySHA256 [32]byte) ([]byte, error) {
	origin, err := NormalizeRelayOrigin(relayOrigin)
	if err != nil {
		return nil, err
	}
	method = strings.ToUpper(strings.TrimSpace(method))
	if method == "" || pathAndQuery == "" || timestamp == "" || nonce == "" {
		return nil, ErrInvalidInput
	}
	return cat(
		field([]byte(origin)),
		field([]byte(method)),
		field([]byte(pathAndQuery)),
		field([]byte(timestamp)),
		field([]byte(nonce)),
		bodySHA256[:],
	), nil
}

// OwnerAuthHeaders are the HTTP headers required for mailbox owner authentication.
type OwnerAuthHeaders struct {
	PubKey    string
	Timestamp string
	Nonce     string
	Sig       string
}

// SignOwnerAuth produces Owner Auth headers for an HTTP request.
func SignOwnerAuth(owner Signer, relayOrigin, method, pathAndQuery string, body []byte, now time.Time, nonce string) (OwnerAuthHeaders, error) {
	if owner == nil {
		return OwnerAuthHeaders{}, ErrInvalidInput
	}
	if nonce == "" {
		var nb [16]byte
		if _, err := rand.Read(nb[:]); err != nil {
			return OwnerAuthHeaders{}, err
		}
		nonce = b64.EncodeToString(nb[:])
	}
	ts := strconv.FormatInt(now.Unix(), 10)
	bodySum := sha256.Sum256(body)
	payload, err := OwnerAuthPayload(relayOrigin, method, pathAndQuery, ts, nonce, bodySum)
	if err != nil {
		return OwnerAuthHeaders{}, err
	}
	sig := owner.SignRelayAuth(payload)
	return OwnerAuthHeaders{
		PubKey:    b64.EncodeToString(owner.Public()),
		Timestamp: ts,
		Nonce:     nonce,
		Sig:       b64.EncodeToString(sig),
	}, nil
}

// VerifyOwnerAuth verifies Owner Auth headers against expectedMailboxID and now.
// Returns the decoded 32-byte Ed25519 public key on success.
func VerifyOwnerAuth(relayOrigin, expectedMailboxID, method, pathAndQuery string, body []byte, hdr OwnerAuthHeaders, now time.Time) ([]byte, error) {
	pk, err := b64.DecodeString(hdr.PubKey)
	if err != nil || len(pk) != ed25519.PublicKeySize {
		return nil, ErrBadKey
	}
	sig, err := b64.DecodeString(hdr.Sig)
	if err != nil || len(sig) != ed25519.SignatureSize {
		return nil, ErrBadSignature
	}
	ts, err := strconv.ParseInt(hdr.Timestamp, 10, 64)
	if err != nil {
		return nil, ErrExpired
	}
	skew := now.Unix() - ts
	if skew < 0 {
		skew = -skew
	}
	if time.Duration(skew)*time.Second > OwnerAuthMaxSkew {
		return nil, ErrExpired
	}
	if len(hdr.Nonce) < 8 || len(hdr.Nonce) > 128 {
		return nil, ErrInvalidInput
	}
	derived, err := DeriveMailboxID(relayOrigin, pk)
	if err != nil || subtle.ConstantTimeCompare([]byte(derived), []byte(expectedMailboxID)) != 1 {
		return nil, ErrBadKey
	}
	bodySum := sha256.Sum256(body)
	payload, err := OwnerAuthPayload(relayOrigin, method, pathAndQuery, hdr.Timestamp, hdr.Nonce, bodySum)
	if err != nil {
		return nil, err
	}
	full := cat([]byte("unic/relay-auth/v1\x00"), payload)
	if !ed25519.Verify(pk, full, sig) {
		return nil, ErrBadSignature
	}
	return pk, nil
}

// ---- Single-chunk binary frame ----

// EncodeSingleFrame packs u32_be(len(envelope_json)) || envelope_json || ciphertext0.
func EncodeSingleFrame(env Envelope, ciphertext0 []byte) ([]byte, error) {
	envJSON, err := json.Marshal(env)
	if err != nil {
		return nil, err
	}
	if len(envJSON) == 0 || len(envJSON) > MaxEnvelopeJSONBytes {
		return nil, ErrBadEnvelope
	}
	out := make([]byte, 4+len(envJSON)+len(ciphertext0))
	binary.BigEndian.PutUint32(out[0:4], uint32(len(envJSON)))
	copy(out[4:4+len(envJSON)], envJSON)
	copy(out[4+len(envJSON):], ciphertext0)
	return out, nil
}

// DecodeSingleFrame unpacks u32_be(len(envelope_json)) || envelope_json || ciphertext0.
func DecodeSingleFrame(frame []byte) (Envelope, []byte, error) {
	if len(frame) < 4 {
		return Envelope{}, nil, ErrBadEnvelope
	}
	n := binary.BigEndian.Uint32(frame[0:4])
	if n == 0 || int(n) > MaxEnvelopeJSONBytes || len(frame) < 4+int(n) {
		return Envelope{}, nil, ErrBadEnvelope
	}
	var env Envelope
	if err := json.Unmarshal(frame[4:4+n], &env); err != nil {
		return Envelope{}, nil, ErrBadEnvelope
	}
	ct := make([]byte, len(frame)-4-int(n))
	copy(ct, frame[4+n:])
	return env, ct, nil
}
