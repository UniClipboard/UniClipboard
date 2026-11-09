// Package identity implements the unified device identity and pairing
// protocol v1 (docs/design/identity-protocol.md): the device key and its
// storage, DeviceID, the fingerprint, the P2P handshake proofs, the pairing
// token proof and object-store author signatures. It knows nothing of
// transports; backends only carry the bytes defined here.
package identity

import (
	"crypto/ed25519"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base32"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
)

// Protocol versions this implementation speaks.
var Versions = []byte{1}

// Kind is the transport kind folded into the transcript.
type Kind byte

const KindPeerSessions Kind = 1

// Role byte of a proof signer.
type Role byte

const (
	RoleInitiator Role = 'I'
	RoleResponder Role = 'R'
)

// Stable error codes (identity-protocol §8).
var (
	ErrUnsupportedVersion = errors.New("unsupported_version")
	ErrBadProof           = errors.New("bad_proof")
	ErrIdentityConflict   = errors.New("identity_conflict")
	ErrBadPairProof       = errors.New("bad_pair_proof")
	ErrBadIdentityFile    = errors.New("bad_identity_file")
	ErrNoIdentity         = errors.New("no_identity")
)

const (
	MaxHello   = 128
	MaxBinding = 256
)

func field(b []byte) []byte {
	out := make([]byte, 2, 2+len(b))
	binary.BigEndian.PutUint16(out, uint16(len(b)))
	return append(out, b...)
}

func cat(parts ...[]byte) []byte {
	var out []byte
	for _, p := range parts {
		out = append(out, p...)
	}
	return out
}

// DeviceID derives the 26-character device ID from a public key.
func DeviceID(pk []byte) string {
	sum := sha256.Sum256(cat([]byte("unic/device-id/v1\x00"), pk))
	return strings.ToLower(base32.StdEncoding.WithPadding(base32.NoPadding).EncodeToString(sum[:16]))
}

// CheckID reports identity_conflict when id does not belong to pk.
func CheckID(id string, pk []byte) error {
	if len(pk) != ed25519.PublicKeySize || DeviceID(pk) != id {
		return ErrIdentityConflict
	}
	return nil
}

// Fingerprint is the 128-bit value both devices compute alike, as eight
// groups of four upper-case hex digits.
func Fingerprint(a, b []byte) string {
	if string(a) > string(b) {
		a, b = b, a
	}
	sum := sha256.Sum256(cat([]byte("unic/fingerprint/v1\x00"), a, b))
	h := strings.ToUpper(hex.EncodeToString(sum[:16]))
	groups := make([]string, 0, 8)
	for i := 0; i < len(h); i += 4 {
		groups = append(groups, h[i:i+4])
	}
	return strings.Join(groups, " ")
}

// ---- the device key ----

// Identity is this device's key pair. The private key never leaves the Engine.
type Identity struct {
	priv ed25519.PrivateKey
}

// FromSeed builds an identity from a 32-byte seed (tests, vectors).
func FromSeed(seed []byte) (*Identity, error) {
	if len(seed) != ed25519.SeedSize {
		return nil, ErrBadIdentityFile
	}
	return &Identity{priv: ed25519.NewKeyFromSeed(seed)}, nil
}

func (i *Identity) Public() []byte   { return []byte(i.priv.Public().(ed25519.PublicKey)) }
func (i *Identity) DeviceID() string { return DeviceID(i.Public()) }

const (
	fileName   = "device.key"
	fileHeader = "UNIC-ID1\n"
)

// Load opens dir/device.key and never creates it. A missing file is
// ErrNoIdentity; one that exists but cannot be trusted is ErrBadIdentityFile.
func Load(dir string) (*Identity, error) {
	id, _, err := open(dir, false)
	return id, err
}

// LoadOrCreate opens dir/device.key, creating a fresh identity only when the
// file does not exist. A file that exists but cannot be trusted is an error;
// a new identity is never generated in its place.
func LoadOrCreate(dir string) (id *Identity, created bool, err error) {
	return open(dir, true)
}

func open(dir string, create bool) (id *Identity, created bool, err error) {
	path := filepath.Join(dir, fileName)
	if !create {
		if _, err := os.Lstat(path); errors.Is(err, fs.ErrNotExist) {
			return nil, false, ErrNoIdentity
		}
	} else if err := os.MkdirAll(dir, 0o700); err != nil {
		return nil, false, fmt.Errorf("identity: %w", err)
	}
	if fi, err := os.Lstat(dir); err != nil || fi.Mode()&fs.ModeSymlink != 0 || !fi.IsDir() {
		return nil, false, ErrBadIdentityFile
	}
	if runtime.GOOS != "windows" {
		if fi, _ := os.Stat(dir); fi.Mode().Perm()&0o077 != 0 {
			return nil, false, ErrBadIdentityFile
		}
	}
	fi, err := os.Lstat(path)
	switch {
	case errors.Is(err, fs.ErrNotExist):
		if !create {
			return nil, false, ErrNoIdentity
		}
		seed := make([]byte, ed25519.SeedSize)
		if _, err := rand.Read(seed); err != nil {
			return nil, false, err
		}
		id, _ := FromSeed(seed)
		if err := writeAtomic(dir, path, []byte(fileHeader+base64.StdEncoding.EncodeToString(seed)+"\n")); err != nil {
			return nil, false, fmt.Errorf("identity: %w", err)
		}
		return id, true, nil
	case err != nil:
		return nil, false, fmt.Errorf("identity: %w", err)
	}
	if !fi.Mode().IsRegular() || (runtime.GOOS != "windows" && fi.Mode().Perm()&0o077 != 0) {
		return nil, false, ErrBadIdentityFile
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		return nil, false, fmt.Errorf("identity: %w", err)
	}
	s := string(raw)
	if !strings.HasPrefix(s, fileHeader) || !strings.HasSuffix(s, "\n") {
		return nil, false, ErrBadIdentityFile
	}
	seed, err := base64.StdEncoding.Strict().DecodeString(strings.TrimSuffix(strings.TrimPrefix(s, fileHeader), "\n"))
	if err != nil {
		return nil, false, ErrBadIdentityFile
	}
	id, err = FromSeed(seed)
	return id, false, err
}

func writeAtomic(dir, path string, data []byte) error {
	tmp, err := os.CreateTemp(dir, ".device.key.*")
	if err != nil {
		return err
	}
	name := tmp.Name()
	ok := false
	defer func() {
		if !ok {
			_ = os.Remove(name)
		}
	}()
	if err := tmp.Chmod(0o600); err != nil && runtime.GOOS != "windows" {
		_ = tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := os.Rename(name, path); err != nil {
		return err
	}
	ok = true
	if d, err := os.Open(dir); err == nil { // directory fsync; not supported everywhere
		_ = d.Sync()
		_ = d.Close()
	}
	return nil
}

// ---- P2P handshake ----

// Hello is one side's opening message.
type Hello struct {
	Versions []byte
	PK       []byte
	Nonce    []byte
}

// NewHello makes a Hello with a fresh nonce.
func NewHello(pk []byte) (Hello, error) {
	n := make([]byte, 32)
	if _, err := rand.Read(n); err != nil {
		return Hello{}, err
	}
	return Hello{Versions: append([]byte(nil), Versions...), PK: pk, Nonce: n}, nil
}

// Encode is the canonical wire form: u8(n) || versions || pk || nonce.
func (h Hello) Encode() []byte {
	return cat([]byte{byte(len(h.Versions))}, h.Versions, h.PK, h.Nonce)
}

// ParseHello strictly decodes a Hello.
func ParseHello(b []byte) (Hello, error) {
	if len(b) < 1 || len(b) > MaxHello {
		return Hello{}, ErrBadProof
	}
	n := int(b[0])
	if n == 0 || len(b) != 1+n+ed25519.PublicKeySize+32 {
		return Hello{}, ErrBadProof
	}
	return Hello{
		Versions: append([]byte(nil), b[1:1+n]...),
		PK:       append([]byte(nil), b[1+n:1+n+ed25519.PublicKeySize]...),
		Nonce:    append([]byte(nil), b[1+n+ed25519.PublicKeySize:]...),
	}, nil
}

// SelectVersion returns the highest version both sides list.
func SelectVersion(a, b Hello) (byte, error) {
	var best byte
	for _, x := range a.Versions {
		for _, y := range b.Versions {
			if x == y && x > best {
				best = x
			}
		}
	}
	if best == 0 {
		return 0, ErrUnsupportedVersion
	}
	return best, nil
}

// Transcript hashes the whole exchange, tying it to the connection.
func Transcript(kind Kind, contextBinding []byte, helloI, helloR []byte) ([32]byte, error) {
	if len(contextBinding) == 0 || len(contextBinding) > MaxBinding {
		return [32]byte{}, ErrBadProof
	}
	return sha256.Sum256(cat([]byte("unic/transcript/v1\x00"), []byte{byte(kind)},
		field(contextBinding), field(helloI), field(helloR))), nil
}

func proofMsg(role Role, t [32]byte) []byte {
	return cat([]byte("unic/proof/v1\x00"), []byte{byte(role)}, t[:])
}

// Prove signs the transcript for the given role.
func (i *Identity) Prove(role Role, t [32]byte) []byte {
	return ed25519.Sign(i.priv, proofMsg(role, t))
}

// VerifyProof checks the peer's proof against the key from its Hello.
func VerifyProof(pk []byte, role Role, t [32]byte, sig []byte) error {
	if len(pk) != ed25519.PublicKeySize || len(sig) != ed25519.SignatureSize ||
		!ed25519.Verify(pk, proofMsg(role, t), sig) {
		return ErrBadProof
	}
	return nil
}

// PairProof is HMAC(tok, domain || T); the token itself never goes on the wire.
func PairProof(tok []byte, t [32]byte) []byte {
	m := hmac.New(sha256.New, tok)
	m.Write(cat([]byte("unic/pair-token/v1\x00"), t[:]))
	return m.Sum(nil)
}

func VerifyPairProof(tok []byte, t [32]byte, proof []byte) error {
	if subtle.ConstantTimeCompare(PairProof(tok, t), proof) != 1 {
		return ErrBadPairProof
	}
	return nil
}

// ---- object store ----

func objectMsg(repoBinding []byte, authorID string, seq, createdMs uint64, content [32]byte) []byte {
	var n [16]byte
	binary.BigEndian.PutUint64(n[:8], seq)
	binary.BigEndian.PutUint64(n[8:], createdMs)
	return cat([]byte("unic/object/v1\x00"), field(repoBinding), field([]byte(authorID)), n[:], content[:])
}

// SignObject signs an object envelope as its author.
func (i *Identity) SignObject(repoBinding []byte, seq, createdMs uint64, content [32]byte) []byte {
	return ed25519.Sign(i.priv, objectMsg(repoBinding, i.DeviceID(), seq, createdMs, content))
}

// VerifyObject checks an envelope against the author's registered key.
func VerifyObject(pk []byte, repoBinding []byte, seq, createdMs uint64, content [32]byte, sig []byte) error {
	if len(repoBinding) == 0 || len(repoBinding) > MaxBinding || len(pk) != ed25519.PublicKeySize ||
		len(sig) != ed25519.SignatureSize ||
		!ed25519.Verify(pk, objectMsg(repoBinding, DeviceID(pk), seq, createdMs, content), sig) {
		return ErrBadProof
	}
	return nil
}
