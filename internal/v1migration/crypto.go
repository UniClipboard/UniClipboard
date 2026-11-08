// Package v1migration reads the frozen UniClipboard v1 disk formats without
// starting its engine or modifying its data or credential store.
package v1migration

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"unicode/utf8"

	"golang.org/x/crypto/argon2"
	"golang.org/x/crypto/chacha20poly1305"
	"golang.org/x/crypto/hkdf"
)

var (
	ErrUnlock  = errors.New("v1 migration: credentials unavailable or incorrect; supply the old passphrase")
	ErrFormat  = errors.New("v1 migration: unsupported or damaged source format")
	ErrContent = errors.New("v1 migration: content missing or failed authentication")
)

// Rust serializes Vec<u8> as integer arrays, not base64.
type rustBytes []byte

func (b *rustBytes) UnmarshalJSON(data []byte) error {
	var ints []uint16
	if err := json.Unmarshal(data, &ints); err != nil {
		return ErrFormat
	}
	*b = make([]byte, len(ints))
	for i, v := range ints {
		if v > 255 {
			return ErrFormat
		}
		(*b)[i] = byte(v)
	}
	return nil
}

type encrypted struct {
	Version    string    `json:"version"`
	AEAD       string    `json:"aead"`
	Nonce      rustBytes `json:"nonce"`
	Ciphertext rustBytes `json:"ciphertext"`
}

func openRaw(key, nonce, ct, aad []byte) ([]byte, error) {
	if len(key) != 32 || len(nonce) != 24 || len(ct) < 16 {
		return nil, ErrFormat
	}
	c, err := chacha20poly1305.NewX(key)
	if err != nil {
		return nil, ErrFormat
	}
	plain, err := c.Open(nil, nonce, ct, aad)
	if err != nil {
		return nil, ErrContent
	}
	return plain, nil
}

func (e encrypted) open(key, aad []byte) ([]byte, error) {
	if e.Version != "V1" || e.AEAD != "XChaCha20Poly1305" {
		return nil, ErrFormat
	}
	return openRaw(key, e.Nonce, e.Ciphertext, aad)
}

func readJSON(path string, out any) error {
	data, err := readBounded(path, 8<<20)
	if err != nil {
		return ErrFormat
	}
	defer clear(data)
	if json.Unmarshal(data, out) != nil {
		return ErrFormat
	}
	return nil
}

func readBounded(path string, limit int64) ([]byte, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, ErrContent
	}
	defer func() { _ = f.Close() }()
	data, err := io.ReadAll(io.LimitReader(f, limit+1))
	if err != nil || int64(len(data)) > limit {
		clear(data)
		return nil, ErrContent
	}
	return data, nil
}

// A small bounded decoder for the exact postcard structs used by v1.
type postcard struct {
	data []byte
	pos  int
	err  error
}

func (p *postcard) take(n int) []byte {
	if p.err != nil || n < 0 || n > len(p.data)-p.pos {
		p.err = ErrFormat
		return nil
	}
	b := p.data[p.pos : p.pos+n]
	p.pos += n
	return b
}
func (p *postcard) number() uint64 {
	if p.err != nil {
		return 0
	}
	n, read := binary.Uvarint(p.data[p.pos:])
	if read <= 0 {
		p.err = ErrFormat
		return 0
	}
	p.pos += read
	return n
}
func (p *postcard) vector() []byte {
	n := p.number()
	if n > uint64(len(p.data)) {
		p.err = ErrFormat
		return nil
	}
	return p.take(int(n))
}
func (p *postcard) text() string {
	b := p.vector()
	if !utf8.Valid(b) {
		p.err = ErrFormat
	}
	return string(b)
}
func (p *postcard) done() error {
	if p.err != nil || p.pos != len(p.data) {
		return ErrFormat
	}
	return nil
}

type contentKey struct {
	group, space string
	epoch        uint64
	key          []byte
}
type cryptoState struct {
	master                       []byte
	keys                         map[string]contentKey
	database, blobRoot, sourceID string
	v3                           bool
}

func derive(key, salt, info []byte) []byte {
	b := make([]byte, 32)
	_, _ = io.ReadFull(hkdf.New(sha256.New, key, salt, info), b)
	return b
}
func fieldBE(dst, b []byte) []byte {
	dst = binary.BigEndian.AppendUint64(dst, uint64(len(b)))
	return append(dst, b...)
}

// SecretReader returns raw credential bytes. It must never write credentials.
type SecretReader func(service, account string) ([]byte, error)

func unlock(root, service string, password []byte, secrets SecretReader) (*cryptoState, error) {
	var slot struct {
		Version string `json:"version"`
		Scope   struct {
			ProfileID string `json:"profile_id"`
		} `json:"scope"`
		Salt rustBytes `json:"salt"`
		KDF  struct {
			Alg    string `json:"alg"`
			Params struct {
				Mem      uint32 `json:"mem_kib"`
				Iters    uint32 `json:"iters"`
				Parallel uint32 `json:"parallelism"`
			} `json:"params"`
		} `json:"kdf"`
		Wrapped encrypted `json:"wrapped_master_key"`
	}
	vault := filepath.Join(root, "vault")
	if err := readJSON(filepath.Join(vault, "keyslot.json"), &slot); err != nil {
		return nil, err
	}
	if slot.Version != "V1" || slot.Scope.ProfileID == "" {
		return nil, ErrFormat
	}
	var kek []byte
	if len(password) > 0 {
		k := slot.KDF
		if k.Alg != "Argon2id" || len(slot.Salt) < 8 || k.Params.Mem < 8*k.Params.Parallel || k.Params.Mem > 1<<20 || k.Params.Iters < 1 || k.Params.Iters > 20 || k.Params.Parallel < 1 || k.Params.Parallel > 64 {
			return nil, ErrFormat
		}
		kek = argon2.IDKey(password, slot.Salt, k.Params.Iters, k.Params.Mem, uint8(k.Params.Parallel), 32)
	} else if secrets != nil {
		kek, _ = secrets(service, "kek:v1:profile:"+slot.Scope.ProfileID)
	}
	defer clear(kek)
	master, err := slot.Wrapped.open(kek, nil)
	if err != nil || len(master) != 32 {
		clear(master)
		return nil, ErrUnlock
	}
	s := &cryptoState{master: master, keys: map[string]contentKey{}, database: filepath.Join(root, "uniclipboard.db"), blobRoot: filepath.Join(root, "blobs")}
	fail := func(err error) (*cryptoState, error) { s.close(); return nil, err }
	// Keyslot and scope identify the source even after its directory is moved.
	id := sha256.New()
	id.Write([]byte("unic-v1-source\000" + slot.Scope.ProfileID))
	id.Write(master)
	s.sourceID = hex.EncodeToString(id.Sum(nil))
	_, manifestErr := os.Stat(filepath.Join(vault, ".active-space-manifest-v2"))
	if os.IsNotExist(manifestErr) {
		return s, nil
	}
	if manifestErr != nil {
		return fail(ErrFormat)
	}
	var secretFile struct {
		Version int        `json:"format_version"`
		Root    encrypted  `json:"wrapped_root"`
		Pending *encrypted `json:"pending_wrapped_root"`
		Payload encrypted  `json:"encrypted_payload"`
	}
	managed := map[string]rustBytes{}
	if readJSON(filepath.Join(vault, "profile-secrets-v1"), &secretFile) == nil {
		if secretFile.Version != 1 {
			return fail(ErrFormat)
		}
		key, e := secretFile.Root.open(kek, nil)
		if e != nil && secretFile.Pending != nil {
			key, e = secretFile.Pending.open(kek, nil)
		}
		if e != nil {
			return fail(ErrUnlock)
		}
		plain, e := secretFile.Payload.open(key, []byte("uniclipboard/profile-secret-payload/v1"))
		clear(key)
		if e != nil {
			return fail(e)
		}
		var payload struct {
			Version int                  `json:"format_version"`
			Secrets map[string]rustBytes `json:"secrets"`
		}
		e = json.Unmarshal(plain, &payload)
		clear(plain)
		if e != nil || payload.Version != 1 {
			return fail(ErrFormat)
		}
		managed = payload.Secrets
	}
	defer func() {
		for _, b := range managed {
			clear(b)
		}
	}()
	get := func(name string) []byte {
		if b := managed[name]; len(b) > 0 {
			return b
		}
		if secrets != nil {
			b, _ := secrets(service, name)
			managed[name] = b
			return b
		}
		return nil
	}
	lifecycle := postcard{data: get("profile_lifecycle_marker:v1")}
	if lifecycle.number() != 1 {
		return fail(ErrUnlock)
	}
	generation := lifecycle.take(16)
	if lifecycle.number() != 0 || lifecycle.done() != nil {
		return fail(ErrFormat)
	}
	profileKey := get("profile_admission_master_key:v1")
	var manifest encrypted
	if readJSON(filepath.Join(vault, ".active-space-manifest-v2"), &manifest) != nil {
		return fail(ErrFormat)
	}
	aad := append([]byte("uniclipboard/admission-profile-payload/v1\x00"), generation...)
	aad = fieldBE(aad, []byte("active-space-manifest-v2"))
	plain, err := manifest.open(profileKey, aad)
	if err != nil {
		return fail(err)
	}
	defer clear(plain)
	p := postcard{data: plain}
	if p.number() != 3 {
		return fail(ErrFormat)
	}
	space := p.text()
	keygen := p.take(16)
	dataGen := p.take(16)
	controlGen := p.take(16)
	digest := p.take(32)
	expected := []byte("uniclipboard/active-runtime-manifest/v3\x00")
	expected = binary.BigEndian.AppendUint16(expected, 3)
	expected = fieldBE(expected, []byte(space))
	expected = append(expected, keygen...)
	expected = append(expected, dataGen...)
	expected = append(expected, controlGen...)
	sum := sha256.Sum256(expected)
	if p.done() != nil || !bytes.Equal(sum[:], digest) {
		return fail(ErrFormat)
	}
	// Path tokens use BLAKE3, unlike the manifest integrity digest.
	token := generationToken(dataGen)
	s.database = filepath.Join(root, "profile-data-generations", token, "v3-payloads", "profile.sqlite")
	s.blobRoot = filepath.Join(filepath.Dir(s.database), "blobs")
	s.v3 = true
	var catalog encrypted
	if readJSON(filepath.Join(vault, "profile-content-key-vault-v1.json"), &catalog) != nil {
		return fail(ErrFormat)
	}
	catalogPlain, err := catalog.open(get("profile_content_vault_key:v1"), append([]byte("uniclipboard/profile-content-key-vault/v1\x00"), generation...))
	if err != nil {
		return fail(err)
	}
	defer clear(catalogPlain)
	if err = s.loadCatalog(catalogPlain); err != nil {
		return fail(err)
	}
	return s, nil
}

func (s *cryptoState) loadCatalog(data []byte) error {
	p := postcard{data: data}
	if p.number() != 1 || p.number() == 0 {
		return ErrFormat
	}
	groups := p.number()
	if groups > 128 {
		return ErrFormat
	}
	for range groups {
		group, space := p.text(), p.text()
		count := p.number()
		if group == "" || space == "" || count == 0 || count > 1024 {
			return ErrFormat
		}
		digest := []byte("uniclipboard/protected-group-catalog/v1\x00")
		digest = fieldBE(digest, []byte(group))
		digest = fieldBE(digest, []byte(space))
		digest = binary.BigEndian.AppendUint64(digest, count)
		for range count {
			id := p.text()
			epoch := p.number()
			key := p.vector()
			if id == "" || len(key) != 32 || len(s.keys) >= 4096 {
				return ErrFormat
			}
			if _, exists := s.keys[id]; exists {
				return ErrFormat
			}
			s.keys[id] = contentKey{group: group, space: space, epoch: epoch, key: bytes.Clone(key)}
			digest = fieldBE(digest, []byte(id))
			digest = binary.BigEndian.AppendUint64(digest, epoch)
			digest = fieldBE(digest, key)
		}
		sum := sha256.Sum256(digest)
		if !bytes.Equal(sum[:], p.take(32)) {
			return ErrFormat
		}
	}
	return p.done()
}

func (s *cryptoState) open(data, businessAAD []byte) ([]byte, error) {
	if !bytes.HasPrefix(data, []byte("UCP3")) {
		var e encrypted
		if json.Unmarshal(data, &e) != nil {
			return nil, ErrFormat
		}
		return e.open(s.master, businessAAD)
	}
	if len(data) < 58 || binary.BigEndian.Uint16(data[4:6]) != 3 || data[6] != 1 {
		return nil, ErrFormat
	}
	n := int(binary.BigEndian.Uint16(data[7:9]))
	epoch := binary.BigEndian.Uint64(data[9:17])
	if n < 1 || 41+n+16 > len(data) {
		return nil, ErrFormat
	}
	id := string(data[41 : 41+n])
	k, ok := s.keys[id]
	if !ok || k.epoch != epoch {
		return nil, ErrContent
	}
	info := fieldBE(nil, []byte("uniclipboard/content-protection/key/v1"))
	info = fieldBE(info, []byte("content"))
	key := derive(k.key, []byte(k.group), info)
	defer clear(key)
	aad := fieldBE(nil, []byte("uniclipboard/content-protection/aad/v1"))
	aad = fieldBE(aad, []byte{0, 3})
	aad = fieldBE(aad, []byte("XChaCha20Poly1305"))
	aad = fieldBE(aad, []byte(k.group))
	aad = fieldBE(aad, []byte(id))
	aad = fieldBE(aad, binary.BigEndian.AppendUint64(nil, epoch))
	aad = fieldBE(aad, []byte("content"))
	aad = fieldBE(aad, businessAAD)
	return openRaw(key, data[17:41], data[41+n:], aad)
}

func (s *cryptoState) close() {
	clear(s.master)
	for _, k := range s.keys {
		clear(k.key)
	}
}
func inlineAAD(event, rep string) []byte {
	return []byte(fmt.Sprintf("uc:inline:v1|%s|%s", event, rep))
}
