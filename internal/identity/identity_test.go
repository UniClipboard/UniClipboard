package identity

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"testing"
)

type vectors struct {
	Keys        map[string]struct{ Seed, Public, Device_ID string }
	Fingerprint string
	P2P         map[string]string
	Object      map[string]string
}

func load(t *testing.T) vectors {
	t.Helper()
	b, err := os.ReadFile("../../docs/contracts/vectors/identity-v1.json")
	if err != nil {
		t.Fatal(err)
	}
	var v vectors
	if err := json.Unmarshal(b, &v); err != nil {
		t.Fatal(err)
	}
	return v
}

func h(t *testing.T, s string) []byte {
	t.Helper()
	b, err := hex.DecodeString(s)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestVectors(t *testing.T) {
	v := load(t)
	a, _ := FromSeed(h(t, v.Keys["A"].Seed))
	b, _ := FromSeed(h(t, v.Keys["B"].Seed))
	if hex.EncodeToString(a.Public()) != v.Keys["A"].Public || a.DeviceID() != v.Keys["A"].Device_ID || b.DeviceID() != v.Keys["B"].Device_ID {
		t.Fatal("key derivation differs")
	}
	if Fingerprint(a.Public(), b.Public()) != v.Fingerprint || Fingerprint(b.Public(), a.Public()) != v.Fingerprint {
		t.Fatal("fingerprint differs")
	}
	p := v.P2P
	hi, hr := h(t, p["hello_initiator"]), h(t, p["hello_responder"])
	if x, err := ParseHello(hi); err != nil || hex.EncodeToString(x.Encode()) != p["hello_initiator"] {
		t.Fatalf("hello round trip: %v", err)
	}
	T, err := Transcript(KindPeerSessions, h(t, p["context_binding"]), hi, hr)
	if err != nil || hex.EncodeToString(T[:]) != p["transcript"] {
		t.Fatalf("transcript differs: %v", err)
	}
	if hex.EncodeToString(a.Prove(RoleInitiator, T)) != p["proof_initiator"] || hex.EncodeToString(b.Prove(RoleResponder, T)) != p["proof_responder"] {
		t.Fatal("proofs differ")
	}
	if hex.EncodeToString(PairProof(h(t, p["pair_token"]), T)) != p["pair_token_proof"] {
		t.Fatal("pair proof differs")
	}
	o := v.Object
	var content [32]byte
	copy(content[:], h(t, o["content_sha256"]))
	seq, _ := strconv.ParseUint(o["seq"], 10, 64)
	created, _ := strconv.ParseUint(o["created_ms"], 10, 64)
	if hex.EncodeToString(a.SignObject(h(t, o["repo_binding"]), seq, created, content)) != o["signature"] {
		t.Fatal("object signature differs")
	}
}

func TestProofRejections(t *testing.T) {
	v := load(t)
	a, _ := FromSeed(h(t, v.Keys["A"].Seed))
	b, _ := FromSeed(h(t, v.Keys["B"].Seed))
	p := v.P2P
	hi, hr, ctx := h(t, p["hello_initiator"]), h(t, p["hello_responder"]), h(t, p["context_binding"])
	T, _ := Transcript(KindPeerSessions, ctx, hi, hr)
	sigI := h(t, p["proof_initiator"])

	if err := VerifyProof(a.Public(), RoleInitiator, T, sigI); err != nil {
		t.Fatal(err)
	}
	// reflected: initiator's proof offered as the responder's
	if VerifyProof(a.Public(), RoleResponder, T, sigI) == nil {
		t.Fatal("role not bound")
	}
	// wrong signer
	if VerifyProof(b.Public(), RoleInitiator, T, sigI) == nil {
		t.Fatal("wrong key accepted")
	}
	// another connection
	ctx2 := append([]byte(nil), ctx...)
	ctx2[0] ^= 1
	T2, _ := Transcript(KindPeerSessions, ctx2, hi, hr)
	if VerifyProof(a.Public(), RoleInitiator, T2, sigI) == nil {
		t.Fatal("proof reused on another connection")
	}
	// downgrade: tampered version list changes the transcript
	hi2 := append([]byte(nil), hi...)
	hi2[1] = 9
	T3, _ := Transcript(KindPeerSessions, ctx, hi2, hr)
	if VerifyProof(a.Public(), RoleInitiator, T3, sigI) == nil {
		t.Fatal("tampered hello accepted")
	}
	if T4, _ := Transcript(2, ctx, hi, hr); VerifyProof(a.Public(), RoleInitiator, T4, sigI) == nil {
		t.Fatal("kind not bound")
	}
	// pair token
	tok := h(t, p["pair_token"])
	if err := VerifyPairProof(tok, T, h(t, p["pair_token_proof"])); err != nil {
		t.Fatal(err)
	}
	if !errors.Is(VerifyPairProof(tok, T2, h(t, p["pair_token_proof"])), ErrBadPairProof) {
		t.Fatal("pair proof reused")
	}
	// object fields are bound
	o := v.Object
	var content [32]byte
	copy(content[:], h(t, o["content_sha256"]))
	sig := h(t, o["signature"])
	repo := h(t, o["repo_binding"])
	if err := VerifyObject(a.Public(), repo, 7, 1794000000000, content, sig); err != nil {
		t.Fatal(err)
	}
	other := sha256.Sum256([]byte("x"))
	if VerifyObject(a.Public(), repo, 8, 1794000000000, content, sig) == nil ||
		VerifyObject(a.Public(), append([]byte{1}, repo...), 7, 1794000000000, content, sig) == nil ||
		VerifyObject(a.Public(), repo, 7, 1794000000000, other, sig) == nil {
		t.Fatal("object field not bound")
	}
}

func TestVersionAndIDChecks(t *testing.T) {
	v := load(t)
	a, _ := FromSeed(h(t, v.Keys["A"].Seed))
	b, _ := FromSeed(h(t, v.Keys["B"].Seed))
	ha, _ := NewHello(a.Public())
	hb, _ := NewHello(b.Public())
	if ver, err := SelectVersion(ha, hb); err != nil || ver != 1 {
		t.Fatal(ver, err)
	}
	hb.Versions = []byte{7}
	if _, err := SelectVersion(ha, hb); !errors.Is(err, ErrUnsupportedVersion) {
		t.Fatal(err)
	}
	if err := CheckID(a.DeviceID(), b.Public()); !errors.Is(err, ErrIdentityConflict) {
		t.Fatal("same ID with another key accepted")
	}
	if _, err := ParseHello([]byte{1, 1}); err == nil {
		t.Fatal("short hello accepted")
	}
}

func TestStorage(t *testing.T) {
	dir := filepath.Join(t.TempDir(), "identity")
	id, created, err := LoadOrCreate(dir)
	if err != nil || !created {
		t.Fatal(created, err)
	}
	again, created, err := LoadOrCreate(dir)
	if err != nil || created || again.DeviceID() != id.DeviceID() {
		t.Fatalf("reload: %v %v", created, err)
	}
	path := filepath.Join(dir, fileName)
	if runtime.GOOS != "windows" {
		if fi, _ := os.Stat(path); fi.Mode().Perm() != 0o600 {
			t.Fatalf("mode %v", fi.Mode())
		}
		if err := os.Chmod(path, 0o644); err != nil {
			t.Fatal(err)
		}
		if _, _, err := LoadOrCreate(dir); !errors.Is(err, ErrBadIdentityFile) {
			t.Fatalf("loose permissions: %v", err)
		}
		_ = os.Chmod(path, 0o600)
	}
	// A damaged file is never replaced by a fresh identity.
	for _, bad := range []string{"", "garbage", fileHeader + "!!!\n", fileHeader + "AAAA\n"} {
		if err := os.WriteFile(path, []byte(bad), 0o600); err != nil {
			t.Fatal(err)
		}
		if _, created, err := LoadOrCreate(dir); err == nil || created {
			t.Fatalf("accepted %q", bad)
		}
		if b, _ := os.ReadFile(path); string(b) != bad {
			t.Fatal("damaged file was overwritten")
		}
	}
	// A symlink is refused.
	if runtime.GOOS != "windows" {
		_ = os.Remove(path)
		target := filepath.Join(t.TempDir(), "k")
		_ = os.WriteFile(target, []byte(fileHeader), 0o600)
		if err := os.Symlink(target, path); err != nil {
			t.Fatal(err)
		}
		if _, _, err := LoadOrCreate(dir); err == nil {
			t.Fatal("followed a symlink")
		}
	}
}
