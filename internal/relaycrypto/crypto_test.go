package relaycrypto

import (
	"bytes"
	"crypto/ecdh"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/identity"
	"github.com/uniclipboard/unic/internal/wire"
)

func mustHex(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatalf("hex %q: %v", s, err)
	}
	return b
}

func TestEd25519ToX25519Agreement(t *testing.T) {
	a, err := identity.FromSeed(bytes.Repeat([]byte{0x01}, 32))
	if err != nil {
		t.Fatal(err)
	}
	b, err := identity.FromSeed(bytes.Repeat([]byte{0x02}, 32))
	if err != nil {
		t.Fatal(err)
	}
	xaPriv, err := a.RelayX25519Key()
	if err != nil {
		t.Fatal(err)
	}
	xbPriv, err := b.RelayX25519Key()
	if err != nil {
		t.Fatal(err)
	}
	xaPub, err := Ed25519PublicToX25519(a.Public())
	if err != nil {
		t.Fatal(err)
	}
	xbPub, err := Ed25519PublicToX25519(b.Public())
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(xaPriv.PublicKey().Bytes(), xaPub.Bytes()) {
		t.Fatalf("A derived X25519 pub mismatch: priv=%x pub=%x", xaPriv.PublicKey().Bytes(), xaPub.Bytes())
	}
	if !bytes.Equal(xbPriv.PublicKey().Bytes(), xbPub.Bytes()) {
		t.Fatalf("B derived X25519 pub mismatch: priv=%x pub=%x", xbPriv.PublicKey().Bytes(), xbPub.Bytes())
	}
	s1, err := xaPriv.ECDH(xbPub)
	if err != nil {
		t.Fatal(err)
	}
	s2, err := xbPriv.ECDH(xaPub)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(s1, s2) {
		t.Fatalf("ECDH mismatch: %x != %x", s1, s2)
	}

	// Small-order Ed25519 identity point must be rejected.
	smallOrder := make([]byte, 32)
	smallOrder[0] = 1
	if _, err := Ed25519PublicToX25519(smallOrder); !errors.Is(err, ErrBadKey) {
		t.Fatalf("expected ErrBadKey for small-order point, got %v", err)
	}
}

func TestNormalizeRelayOrigin(t *testing.T) {
	cases := []struct {
		in   string
		want string
		bad  bool
	}{
		{"https://RELAY.UniClipboard.App:443/", "https://relay.uniclipboard.app", false},
		{"https://relay.example.com:8443/v1/sub/", "https://relay.example.com:8443/v1/sub", false},
		{"http://127.0.0.1:80/", "http://127.0.0.1", false},
		{"http://[::1]:8080/", "http://[::1]:8080", false},
		{"ftp://relay.example.com", "", true},
		{"https://user:pass@relay.example.com", "", true},
		{"https://relay.example.com?x=1", "", true},
		{"https://relay.example.com#frag", "", true},
		{"", "", true},
	}
	for _, tc := range cases {
		got, err := NormalizeRelayOrigin(tc.in)
		if tc.bad {
			if err == nil {
				t.Errorf("NormalizeRelayOrigin(%q) = %q, want error", tc.in, got)
			}
			continue
		}
		if err != nil || got != tc.want {
			t.Errorf("NormalizeRelayOrigin(%q) = (%q, %v), want (%q, nil)", tc.in, got, err, tc.want)
		}
	}
}

func TestEnvelopeChunkRoundTripAndTamperRejection(t *testing.T) {
	sender, _ := identity.FromSeed(bytes.Repeat([]byte{0x01}, 32))
	recipient, _ := identity.FromSeed(bytes.Repeat([]byte{0x02}, 32))
	other, _ := identity.FromSeed(bytes.Repeat([]byte{0x03}, 32))

	origin := "https://relay.uniclipboard.app"
	transferID := "0123456789abcdef0123456789abcdef"
	now := time.UnixMilli(1764124800000)
	cek := bytes.Repeat([]byte{0x42}, 32)
	chunks := [][]byte{[]byte("chunk-zero-payload"), []byte("chunk-one-final")}

	cc, err := NewChunkCipher(cek, transferID, sender.DeviceID(), recipient.DeviceID(), len(chunks))
	if err != nil {
		t.Fatal(err)
	}
	cts := make([][]byte, len(chunks))
	hashes := make([][32]byte, len(chunks))
	var totalCT int64
	for i, pt := range chunks {
		ct, h, err := cc.SealChunk(i, pt)
		if err != nil {
			t.Fatal(err)
		}
		cts[i], hashes[i] = ct, h
		totalCT += int64(len(ct))
	}

	env, err := SealEnvelope(sender, SealEnvelopeInput{
		RelayOrigin:          origin,
		TransferID:           transferID,
		RecipientPK:          recipient.Public(),
		CreatedMs:            now.UnixMilli(),
		ExpiresMs:            now.Add(DefaultTTL).UnixMilli(),
		ChunkSize:            DefaultChunkSize,
		TotalChunks:          len(chunks),
		TotalCiphertextBytes: totalCT,
		ChunkHashes:          hashes,
		CEK:                  cek,
	})
	if err != nil {
		t.Fatal(err)
	}

	ve, err := VerifyEnvelope(origin, env, recipient.DeviceID(), sender.Public(), now.Add(time.Minute))
	if err != nil {
		t.Fatalf("VerifyEnvelope: %v", err)
	}
	gotCEK, err := OpenCEK(recipient, origin, ve)
	if err != nil {
		t.Fatalf("OpenCEK: %v", err)
	}
	if !bytes.Equal(gotCEK, cek) {
		t.Fatalf("CEK mismatch")
	}

	rc, err := NewChunkCipher(gotCEK, transferID, sender.DeviceID(), recipient.DeviceID(), len(chunks))
	if err != nil {
		t.Fatal(err)
	}
	for i := range chunks {
		pt, err := rc.OpenChunk(i, cts[i], ve.ChunkHashes[i])
		if err != nil {
			t.Fatalf("OpenChunk(%d): %v", i, err)
		}
		if !bytes.Equal(pt, chunks[i]) {
			t.Fatalf("chunk %d mismatch: %q != %q", i, pt, chunks[i])
		}
	}

	// Negative tests:
	// 1. Wrong relay origin
	if _, err := VerifyEnvelope("https://evil.example.com", env, recipient.DeviceID(), sender.Public(), now); !errors.Is(err, ErrBadSignature) {
		t.Fatalf("wrong origin: got %v, want ErrBadSignature", err)
	}
	// 2. Expired envelope
	if _, err := VerifyEnvelope(origin, env, recipient.DeviceID(), sender.Public(), now.Add(DefaultTTL+time.Second)); !errors.Is(err, ErrExpired) {
		t.Fatalf("expired: got %v, want ErrExpired", err)
	}
	// 3. Wrong recipient opening CEK
	if _, err := OpenCEK(other, origin, ve); !errors.Is(err, ErrDecryptFailed) {
		t.Fatalf("wrong recipient: got %v, want ErrDecryptFailed", err)
	}
	// 4. Swapped chunk index
	if _, err := rc.OpenChunk(0, cts[1], sha256.Sum256(cts[1])); !errors.Is(err, ErrDecryptFailed) {
		t.Fatalf("swapped chunk index: got %v, want ErrDecryptFailed", err)
	}
	// 5. Tampered ciphertext fails hash check
	badCT := append([]byte(nil), cts[0]...)
	badCT[0] ^= 0xff
	if _, err := rc.OpenChunk(0, badCT, ve.ChunkHashes[0]); !errors.Is(err, ErrChunkHash) {
		t.Fatalf("tampered ct: got %v, want ErrChunkHash", err)
	}
	// 6. Tampered wrapped_cek fails signature
	badEnv := env
	badEnv.WrappedCEK = b64.EncodeToString(bytes.Repeat([]byte{0x99}, WrappedSize))
	if _, err := VerifyEnvelope(origin, badEnv, recipient.DeviceID(), sender.Public(), now); !errors.Is(err, ErrBadSignature) {
		t.Fatalf("tampered wrapped_cek: got %v, want ErrBadSignature", err)
	}
}

func TestSingleFrameAndPlaintextHeaderAndBitmap(t *testing.T) {
	clip := wire.Clip{
		T:      "clip",
		ID:     "0123456789abcdef0123456789abcdef",
		Origin: "q5opsuq3efr3sxg3vlqcgetwvu",
		TS:     1764124800000,
		Hash:   "abc",
		Parts:  []wire.PartMeta{{Kind: "fmt", Name: "text/plain", Size: 5}},
	}
	hdrBytes, err := EncodePlaintextHeader(clip)
	if err != nil {
		t.Fatal(err)
	}
	got, err := ReadPlaintextHeader(bytes.NewReader(append(hdrBytes, []byte("hello")...)))
	if err != nil {
		t.Fatal(err)
	}
	if got.ID != clip.ID || len(got.Parts) != 1 || got.Parts[0].Name != "text/plain" {
		t.Fatalf("header mismatch: %+v", got)
	}

	env := Envelope{V: 1, TransferID: clip.ID}
	frame, err := EncodeSingleFrame(env, []byte{1, 2, 3, 4})
	if err != nil {
		t.Fatal(err)
	}
	env2, ct2, err := DecodeSingleFrame(frame)
	if err != nil {
		t.Fatal(err)
	}
	if env2.TransferID != clip.ID || !bytes.Equal(ct2, []byte{1, 2, 3, 4}) {
		t.Fatalf("single frame mismatch")
	}

	bm, err := NewBitmap(10)
	if err != nil {
		t.Fatal(err)
	}
	SetBit(bm, 0)
	SetBit(bm, 3)
	SetBit(bm, 9)
	if CountBits(bm, 10) != 3 {
		t.Fatalf("CountBits = %d, want 3", CountBits(bm, 10))
	}
	enc := EncodeBitmap(bm)
	dec, err := DecodeBitmap(enc, 10)
	if err != nil || !HasBit(dec, 0) || !HasBit(dec, 3) || !HasBit(dec, 9) || HasBit(dec, 1) {
		t.Fatalf("DecodeBitmap mismatch: %v %x", err, dec)
	}
	// Non-zero trailing bit must fail.
	bm[1] |= 0x80
	if _, err := DecodeBitmap(EncodeBitmap(bm), 10); err == nil {
		t.Fatalf("expected trailing bit rejection")
	}
}

func TestOwnerAuthRoundTrip(t *testing.T) {
	owner, _ := identity.FromSeed(bytes.Repeat([]byte{0x02}, 32))
	origin := "https://relay.uniclipboard.app"
	mb, err := DeriveMailboxID(origin, owner.Public())
	if err != nil {
		t.Fatal(err)
	}
	now := time.Unix(1764124800, 0)
	path := "/v1/relay/mailboxes/" + mb + "/transfers"
	hdr, err := SignOwnerAuth(owner, origin, "GET", path, nil, now, "fixed-nonce-123456")
	if err != nil {
		t.Fatal(err)
	}
	pk, err := VerifyOwnerAuth(origin, mb, "GET", path, nil, hdr, now.Add(5*time.Second))
	if err != nil || !bytes.Equal(pk, owner.Public()) {
		t.Fatalf("VerifyOwnerAuth failed: %v", err)
	}
	// Skew > 60s rejected
	if _, err := VerifyOwnerAuth(origin, mb, "GET", path, nil, hdr, now.Add(61*time.Second)); !errors.Is(err, ErrExpired) {
		t.Fatalf("expected ErrExpired, got %v", err)
	}
	// Wrong path rejected
	if _, err := VerifyOwnerAuth(origin, mb, "GET", path+"/other", nil, hdr, now); !errors.Is(err, ErrBadSignature) {
		t.Fatalf("expected ErrBadSignature, got %v", err)
	}
}

type hybridVectorFile struct {
	Version     int    `json:"version"`
	RelayOrigin string `json:"relay_origin"`
	Keys        struct {
		Sender struct {
			Seed     string `json:"seed"`
			Public   string `json:"public"`
			X25519   string `json:"x25519_public"`
			DeviceID string `json:"device_id"`
		} `json:"sender"`
		Recipient struct {
			Seed      string `json:"seed"`
			Public    string `json:"public"`
			X25519    string `json:"x25519_public"`
			DeviceID  string `json:"device_id"`
			MailboxID string `json:"mailbox_id"`
		} `json:"recipient"`
	} `json:"keys"`
	Transfer struct {
		TransferID  string   `json:"transfer_id"`
		CreatedMs   int64    `json:"created_ms"`
		ExpiresMs   int64    `json:"expires_ms"`
		CEK         string   `json:"cek"`
		EphPriv     string   `json:"eph_priv"`
		EphPub      string   `json:"eph_pub"`
		Plaintexts  []string `json:"plaintexts_hex"`
		Ciphertexts []string `json:"ciphertexts_hex"`
		Envelope    Envelope `json:"envelope"`
	} `json:"transfer"`
	OwnerAuth struct {
		Method       string           `json:"method"`
		PathAndQuery string           `json:"path_and_query"`
		BodyHex      string           `json:"body_hex"`
		NowUnix      int64            `json:"now_unix"`
		Nonce        string           `json:"nonce"`
		Headers      OwnerAuthHeaders `json:"headers"`
	} `json:"owner_auth"`
}

func TestHybridRelayVectors(t *testing.T) {
	vecPath := filepath.Join("..", "..", "docs", "contracts", "vectors", "hybrid-relay-v1.json")
	if os.Getenv("UNIC_GENERATE_RELAY_VECTORS") == "1" {
		generateVectors(t, vecPath)
	}
	raw, err := os.ReadFile(vecPath)
	if err != nil {
		t.Fatalf("read vectors: %v", err)
	}
	var v hybridVectorFile
	if err := json.Unmarshal(raw, &v); err != nil {
		t.Fatalf("parse vectors: %v", err)
	}
	sender, err := identity.FromSeed(mustHex(t, v.Keys.Sender.Seed))
	if err != nil {
		t.Fatal(err)
	}
	recipient, err := identity.FromSeed(mustHex(t, v.Keys.Recipient.Seed))
	if err != nil {
		t.Fatal(err)
	}
	if sender.DeviceID() != v.Keys.Sender.DeviceID || recipient.DeviceID() != v.Keys.Recipient.DeviceID {
		t.Fatalf("device id mismatch")
	}
	sx, err := Ed25519PublicToX25519(sender.Public())
	if err != nil || hex.EncodeToString(sx.Bytes()) != v.Keys.Sender.X25519 {
		t.Fatalf("sender x25519 mismatch: got %x want %s", sx.Bytes(), v.Keys.Sender.X25519)
	}
	rx, err := Ed25519PublicToX25519(recipient.Public())
	if err != nil || hex.EncodeToString(rx.Bytes()) != v.Keys.Recipient.X25519 {
		t.Fatalf("recipient x25519 mismatch: got %x want %s", rx.Bytes(), v.Keys.Recipient.X25519)
	}
	mb, err := DeriveMailboxID(v.RelayOrigin, recipient.Public())
	if err != nil || mb != v.Keys.Recipient.MailboxID {
		t.Fatalf("mailbox mismatch: got %s want %s", mb, v.Keys.Recipient.MailboxID)
	}

	cek := mustHex(t, v.Transfer.CEK)
	ephPriv, err := ecdh.X25519().NewPrivateKey(mustHex(t, v.Transfer.EphPriv))
	if err != nil {
		t.Fatal(err)
	}
	cc, err := NewChunkCipher(cek, v.Transfer.TransferID, sender.DeviceID(), recipient.DeviceID(), len(v.Transfer.Plaintexts))
	if err != nil {
		t.Fatal(err)
	}
	hashes := make([][32]byte, len(v.Transfer.Plaintexts))
	var totalCT int64
	for i, ph := range v.Transfer.Plaintexts {
		ct, h, err := cc.SealChunk(i, mustHex(t, ph))
		if err != nil {
			t.Fatal(err)
		}
		if hex.EncodeToString(ct) != v.Transfer.Ciphertexts[i] {
			t.Fatalf("chunk %d ciphertext mismatch:\ngot  %x\nwant %s", i, ct, v.Transfer.Ciphertexts[i])
		}
		hashes[i] = h
		totalCT += int64(len(ct))
	}
	env, err := SealEnvelope(sender, SealEnvelopeInput{
		RelayOrigin:          v.RelayOrigin,
		TransferID:           v.Transfer.TransferID,
		RecipientPK:          recipient.Public(),
		CreatedMs:            v.Transfer.CreatedMs,
		ExpiresMs:            v.Transfer.ExpiresMs,
		ChunkSize:            v.Transfer.Envelope.ChunkSize,
		TotalChunks:          len(v.Transfer.Plaintexts),
		TotalCiphertextBytes: totalCT,
		ChunkHashes:          hashes,
		CEK:                  cek,
		EphemeralPriv:        ephPriv,
	})
	if err != nil {
		t.Fatal(err)
	}
	gotJSON, _ := json.Marshal(env)
	wantJSON, _ := json.Marshal(v.Transfer.Envelope)
	if !bytes.Equal(gotJSON, wantJSON) {
		t.Fatalf("envelope mismatch:\ngot  %s\nwant %s", gotJSON, wantJSON)
	}
	ve, err := VerifyEnvelope(v.RelayOrigin, env, recipient.DeviceID(), sender.Public(), time.UnixMilli(v.Transfer.CreatedMs))
	if err != nil {
		t.Fatalf("VerifyEnvelope vector: %v", err)
	}
	gotCEK, err := OpenCEK(recipient, v.RelayOrigin, ve)
	if err != nil || !bytes.Equal(gotCEK, cek) {
		t.Fatalf("OpenCEK vector failed: %v", err)
	}

	hdr, err := SignOwnerAuth(recipient, v.RelayOrigin, v.OwnerAuth.Method, v.OwnerAuth.PathAndQuery, mustHex(t, v.OwnerAuth.BodyHex), time.Unix(v.OwnerAuth.NowUnix, 0), v.OwnerAuth.Nonce)
	if err != nil {
		t.Fatal(err)
	}
	if hdr != v.OwnerAuth.Headers {
		t.Fatalf("owner auth headers mismatch:\ngot  %+v\nwant %+v", hdr, v.OwnerAuth.Headers)
	}
	if _, err := VerifyOwnerAuth(v.RelayOrigin, mb, v.OwnerAuth.Method, v.OwnerAuth.PathAndQuery, mustHex(t, v.OwnerAuth.BodyHex), hdr, time.Unix(v.OwnerAuth.NowUnix, 0)); err != nil {
		t.Fatalf("VerifyOwnerAuth vector: %v", err)
	}
}

func generateVectors(t *testing.T, vecPath string) {
	t.Helper()
	origin := "https://relay.uniclipboard.app"
	sSeed := bytes.Repeat([]byte{0x01}, 32)
	rSeed := bytes.Repeat([]byte{0x02}, 32)
	sender, err := identity.FromSeed(sSeed)
	if err != nil {
		t.Fatal(err)
	}
	recipient, err := identity.FromSeed(rSeed)
	if err != nil {
		t.Fatal(err)
	}
	sx, err := Ed25519PublicToX25519(sender.Public())
	if err != nil {
		t.Fatal(err)
	}
	rx, err := Ed25519PublicToX25519(recipient.Public())
	if err != nil {
		t.Fatal(err)
	}
	mb, err := DeriveMailboxID(origin, recipient.Public())
	if err != nil {
		t.Fatal(err)
	}
	transferID := "0123456789abcdef0123456789abcdef"
	createdMs := int64(1764124800000)
	expiresMs := int64(1764125400000)
	cek := bytes.Repeat([]byte{0x42}, 32)
	ephRaw := bytes.Repeat([]byte{0x33}, 32)
	ephPriv, err := ecdh.X25519().NewPrivateKey(ephRaw)
	if err != nil {
		t.Fatal(err)
	}
	pts := [][]byte{[]byte("hello-relay-chunk-0"), []byte("hello-relay-chunk-1-final")}
	cc, err := NewChunkCipher(cek, transferID, sender.DeviceID(), recipient.DeviceID(), len(pts))
	if err != nil {
		t.Fatal(err)
	}
	hashes := make([][32]byte, len(pts))
	ctsHex := make([]string, len(pts))
	ptsHex := make([]string, len(pts))
	var totalCT int64
	for i, pt := range pts {
		ct, h, err := cc.SealChunk(i, pt)
		if err != nil {
			t.Fatal(err)
		}
		hashes[i] = h
		ptsHex[i] = hex.EncodeToString(pt)
		ctsHex[i] = hex.EncodeToString(ct)
		totalCT += int64(len(ct))
	}
	env, err := SealEnvelope(sender, SealEnvelopeInput{
		RelayOrigin:          origin,
		TransferID:           transferID,
		RecipientPK:          recipient.Public(),
		CreatedMs:            createdMs,
		ExpiresMs:            expiresMs,
		ChunkSize:            DefaultChunkSize,
		TotalChunks:          len(pts),
		TotalCiphertextBytes: totalCT,
		ChunkHashes:          hashes,
		CEK:                  cek,
		EphemeralPriv:        ephPriv,
	})
	if err != nil {
		t.Fatal(err)
	}
	authPath := "/v1/relay/mailboxes/" + mb + "/transfers"
	authBody := []byte{}
	authNow := int64(1764124800)
	authNonce := "fixed-vector-nonce-01"
	hdr, err := SignOwnerAuth(recipient, origin, "GET", authPath, authBody, time.Unix(authNow, 0), authNonce)
	if err != nil {
		t.Fatal(err)
	}
	var doc hybridVectorFile
	doc.Version = 1
	doc.RelayOrigin = origin
	doc.Keys.Sender.Seed = hex.EncodeToString(sSeed)
	doc.Keys.Sender.Public = hex.EncodeToString(sender.Public())
	doc.Keys.Sender.X25519 = hex.EncodeToString(sx.Bytes())
	doc.Keys.Sender.DeviceID = sender.DeviceID()
	doc.Keys.Recipient.Seed = hex.EncodeToString(rSeed)
	doc.Keys.Recipient.Public = hex.EncodeToString(recipient.Public())
	doc.Keys.Recipient.X25519 = hex.EncodeToString(rx.Bytes())
	doc.Keys.Recipient.DeviceID = recipient.DeviceID()
	doc.Keys.Recipient.MailboxID = mb
	doc.Transfer.TransferID = transferID
	doc.Transfer.CreatedMs = createdMs
	doc.Transfer.ExpiresMs = expiresMs
	doc.Transfer.CEK = hex.EncodeToString(cek)
	doc.Transfer.EphPriv = hex.EncodeToString(ephRaw)
	doc.Transfer.EphPub = hex.EncodeToString(ephPriv.PublicKey().Bytes())
	doc.Transfer.Plaintexts = ptsHex
	doc.Transfer.Ciphertexts = ctsHex
	doc.Transfer.Envelope = env
	doc.OwnerAuth.Method = "GET"
	doc.OwnerAuth.PathAndQuery = authPath
	doc.OwnerAuth.BodyHex = hex.EncodeToString(authBody)
	doc.OwnerAuth.NowUnix = authNow
	doc.OwnerAuth.Nonce = authNonce
	doc.OwnerAuth.Headers = hdr
	b, err := json.MarshalIndent(doc, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(vecPath, append(b, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
