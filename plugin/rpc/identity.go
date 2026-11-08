package rpc

import (
	"encoding/base64"
	"regexp"
	"unicode/utf8"
)

// Identity methods of the peer_sessions profile (identity-protocol §4). The
// Engine owns the device key, the Hello nonces and the verification; the
// backend only carries opaque bytes between the two ends of a connection. All
// binary fields are base64url without padding.
const (
	MethodIdentityHello = "sync_host.identity_hello" // backend→Engine
	MethodIdentityProve = "sync_host.identity_prove" // backend→Engine
	MethodCheckPeer     = "sync_host.check_peer"     // backend→Engine
)

// Identity limits (identity-protocol §8) in decoded bytes.
const (
	MaxIdentityHello   = 128
	MaxContextBinding  = 256
	ProofBytes         = 64 // Ed25519 signature
	PairProofBytes     = 32 // HMAC-SHA256
	MaxPeerNameBytes   = 256
	MaxPeerAddress     = 8192
	MinIdentityHello   = 1 + 1 + 32 + 32 // one version, pk, nonce
	maxIdentityVersion = 2147483647
)

// Identity errors reuse the stable codes: unsupported_version →
// protocol_mismatch, identity_conflict → identity_mismatch, expired →
// resource_expired; the others have their own code.
const (
	CodeBadProof        = "bad_proof"
	CodeBadPairProof    = "bad_pair_proof"
	CodeReplay          = "replay"
	CodeBindingMismatch = "binding_mismatch"
)

var (
	hex64Re = regexp.MustCompile(`^[0-9a-f]{64}$`)
	idRe    = regexp.MustCompile(`^[A-Za-z0-9._:-]+$`)
)

func b64Len(s string, min, max int) bool {
	b, err := base64.RawURLEncoding.Strict().DecodeString(s)
	return err == nil && len(b) >= min && len(b) <= max
}

func validConn(s string) bool { return validID(s) && idRe.MatchString(s) }

func validDirection(s string) bool { return s == "incoming" || s == "outgoing" }

func validAttempt(a *string) bool { return a == nil || hex64Re.MatchString(*a) }

// IdentityHello asks the Engine for its Hello for one connection. The backend
// supplies the context binding both ends compute identically and that is
// unique to this connection; the Engine keeps the nonce until the handshake
// ends or 60 seconds pass. Calling it twice for one connection_id conflicts.
type IdentityHello struct {
	ConnectionID   string  `json:"connection_id"`
	Direction      string  `json:"direction"` // outgoing is the initiator (I), incoming the responder (R)
	AttemptID      *string `json:"attempt_id"`
	ContextBinding string  `json:"context_binding"`
}

func (r IdentityHello) Validate() error {
	if !validConn(r.ConnectionID) || !validDirection(r.Direction) || !validAttempt(r.AttemptID) ||
		!b64Len(r.ContextBinding, 1, MaxContextBinding) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type IdentityHelloResult struct {
	Hello string `json:"hello"`
}

func (r IdentityHelloResult) Validate() error {
	if !b64Len(r.Hello, MinIdentityHello, MaxIdentityHello) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// IdentityProve hands the Engine the peer's Hello and gets back the proof to
// send. Only the transcript of this connection can be signed; there is no
// general signing method. PairProof is non-null only for an outgoing attempt
// that carries a pairing ticket; the token itself never reaches the backend.
type IdentityProve struct {
	ConnectionID string `json:"connection_id"`
	PeerHello    string `json:"peer_hello"`
}

func (r IdentityProve) Validate() error {
	if !validConn(r.ConnectionID) || !b64Len(r.PeerHello, MinIdentityHello, MaxIdentityHello) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type IdentityProveResult struct {
	Proof     string  `json:"proof"`
	PairProof *string `json:"pair_proof"`
}

func (r IdentityProveResult) Validate() error {
	if !b64Len(r.Proof, ProofBytes, ProofBytes) || (r.PairProof != nil && !b64Len(*r.PairProof, PairProofBytes, PairProofBytes)) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// PeerWire is what the peer announced in the transport's own hello. Its
// self-reported device ID is gone: the ID is derived from the proven key.
type PeerWire struct {
	WireVersion int    `json:"wire_version"`
	Name        string `json:"name"`
	Address     string `json:"address"`
}

func (w PeerWire) validate() bool {
	return w.WireVersion >= 0 && w.WireVersion <= maxIdentityVersion && utf8.ValidString(w.Name) && len(w.Name) <= MaxPeerNameBytes &&
		utf8.ValidString(w.Address) && len(w.Address) <= MaxPeerAddress
}

// CheckPeer is called after the peer's Proof arrived. The Engine verifies it
// against the stored transcript and decides; a backend's own claim that a
// peer is verified is never accepted. PeerPairProof is the optional
// HMAC from identity-protocol §4.4.
type CheckPeer struct {
	ConnectionID  string   `json:"connection_id"`
	Direction     string   `json:"direction"`
	AttemptID     *string  `json:"attempt_id"`
	Hello         PeerWire `json:"hello"`
	PeerProof     string   `json:"peer_proof"`
	PeerPairProof *string  `json:"peer_pair_proof"`
}

func (r CheckPeer) Validate() error {
	if !validConn(r.ConnectionID) || !validDirection(r.Direction) || !validAttempt(r.AttemptID) || !r.Hello.validate() ||
		!b64Len(r.PeerProof, ProofBytes, ProofBytes) || (r.PeerPairProof != nil && !b64Len(*r.PeerPairProof, PairProofBytes, PairProofBytes)) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// PeerDecision: allow carries a session token; pending and reject do not.
// PeerID is derived by the Engine from the proven public key.
type PeerDecision struct {
	PeerID        string  `json:"peer_id"`
	Decision      string  `json:"decision"`
	SessionToken  *string `json:"session_token"`
	TrustRevision U64     `json:"trust_revision"`
}

func (r PeerDecision) Validate() error {
	if !validConn(r.PeerID) {
		return NewError(CodeInvalidArgument)
	}
	switch r.Decision {
	case "allow":
		if r.SessionToken == nil || !hex64Re.MatchString(*r.SessionToken) {
			return NewError(CodeInvalidArgument)
		}
	case "pending", "reject":
		if r.SessionToken != nil {
			return NewError(CodeInvalidArgument)
		}
	default:
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// Connect starts an explicit pairing attempt. The pairing token is not part
// of it: the Engine keeps it and proves possession through IdentityProve.
type Connect struct {
	AttemptID      string `json:"attempt_id"`
	Address        string `json:"address"`
	ExpectedPeerID string `json:"expected_peer_id"`
}

func (r Connect) Validate() error {
	if !hex64Re.MatchString(r.AttemptID) || r.Address == "" || len(r.Address) > MaxPeerAddress || !utf8.ValidString(r.Address) ||
		(r.ExpectedPeerID != "" && !validConn(r.ExpectedPeerID)) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}
