package rpc

// Receive admission for peer_sessions backends (sync-backend-contract §5).
const (
	MethodReceiveOpen     = "sync_host.receive_open"
	MethodPeerReceive     = "sync_host.receive"
	MethodPeerReceivePart = "sync_host.receive_part"

	MaxRecvRepBytes   = 64 << 20 // one in-memory representation
	MaxRecvFileBytes  = 2 << 30  // one file
	MaxRecvTotalBytes = 4 << 30  // whole clip
	MaxOpenReceives   = 32       // granted and not yet finished, per binding
	ReceiveOpenTTLMs  = 10_000   // a receive_id must be consumed within this

	DecisionAccept  = "accept"
	DecisionDiscard = "discard"
	OutcomeCommit   = "committed"
	OutcomeDiscard  = "discarded"
)

// ReceiveOpen asks to admit a clip whose header was read from a connected
// session; it precedes reading any body byte.
type ReceiveOpen struct {
	ConnectionID string `json:"connection_id"`
	SessionToken string `json:"session_token"`
	Clip         Clip   `json:"clip"`
}

// Validate applies the protocol's hard limits. A clip failing them is not
// drainable: the backend must close the connection.
func (r ReceiveOpen) Validate() error {
	if !validID(r.ConnectionID) || !hex64Re.MatchString(r.SessionToken) {
		return NewError(CodeInvalidArgument)
	}
	if err := r.Clip.Validate(); err != nil {
		return err
	}
	var total uint64
	for _, p := range r.Clip.Parts {
		limit := uint64(MaxRecvRepBytes)
		if p.Kind == "file" {
			limit = MaxRecvFileBytes
		}
		sz := uint64(p.Size)
		if sz > limit || sz > MaxRecvTotalBytes-total {
			return NewError(CodeInvalidArgument)
		}
		total += sz
	}
	return nil
}

// ReceiveGrant: accept carries the single-use receive_id; discard carries nil
// and tells the backend to drain the declared length and keep the connection.
type ReceiveGrant struct {
	Decision  string  `json:"decision"`
	ReceiveID *string `json:"receive_id"`
}

func (g ReceiveGrant) Validate() error {
	switch {
	case g.Decision == DecisionAccept && g.ReceiveID != nil && hex64Re.MatchString(*g.ReceiveID):
		return nil
	case g.Decision == DecisionDiscard && g.ReceiveID == nil:
		return nil
	}
	return NewError(CodeInvalidArgument)
}

// PeerReceive consumes a receive_id; its response comes after every part and
// the Engine's processing and commit.
type PeerReceive struct {
	ReceiveID string `json:"receive_id"`
}

func (r PeerReceive) Validate() error {
	if !hex64Re.MatchString(r.ReceiveID) {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

// PeerReceivePart declares part Index's stream. A zero-size part has no stream:
// stream_id 0, size 0.
type PeerReceivePart struct {
	ReceiveID string     `json:"receive_id"`
	Index     int        `json:"index"`
	Stream    StreamDesc `json:"stream"`
}

func (p PeerReceivePart) Validate() error {
	if !hex64Re.MatchString(p.ReceiveID) || p.Index < 0 || p.Index >= MaxClipParts || p.Stream.Purpose != PurposeReceivePart {
		return NewError(CodeInvalidArgument)
	}
	return nil
}

type PeerReceiveResult struct {
	Outcome string `json:"outcome"`
}

func (r PeerReceiveResult) Validate() error {
	if r.Outcome != OutcomeCommit && r.Outcome != OutcomeDiscard {
		return NewError(CodeInvalidArgument)
	}
	return nil
}
