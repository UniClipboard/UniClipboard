package rpc

import (
	"encoding/json"
	"regexp"
	"slices"
	"unicode/utf8"
)

// Limits are the negotiated transport bounds.
type Limits struct {
	ControlBytes      int `json:"control_bytes"`
	DataBytes         int `json:"data_bytes"`
	StreamWindowBytes int `json:"stream_window_bytes"`
	MaxStreams        int `json:"max_streams"`
	MaxRequests       int `json:"max_requests"`
}

// DefaultLimits are both the default offer and the hard upper bounds.
var DefaultLimits = Limits{ControlBytes: 1 << 20, DataBytes: 64 << 10, StreamWindowBytes: 1 << 20, MaxStreams: 32, MaxRequests: 128}

const (
	// SessionStreamBudget bounds granted-but-unconsumed stream bytes.
	SessionStreamBudget = 8 << 20
	MinControlBytes     = 4096
	MinDataBytes        = 1024
	MinWindowBytes      = 64 << 10
	MinRequests         = 4
)

// Validate checks l against the schema minimum and maximum values.
func (l Limits) Validate() error {
	h := DefaultLimits
	if l.ControlBytes < MinControlBytes || l.ControlBytes > h.ControlBytes ||
		l.DataBytes < MinDataBytes || l.DataBytes > h.DataBytes ||
		l.StreamWindowBytes < MinWindowBytes || l.StreamWindowBytes > h.StreamWindowBytes ||
		l.MaxStreams < 1 || l.MaxStreams > h.MaxStreams ||
		l.MaxRequests < MinRequests || l.MaxRequests > h.MaxRequests {
		return protoErr("limits out of range")
	}
	return nil
}

// Negotiate returns the element-wise minimum.
func Negotiate(a, b Limits) Limits {
	return Limits{
		ControlBytes: min(a.ControlBytes, b.ControlBytes), DataBytes: min(a.DataBytes, b.DataBytes),
		StreamWindowBytes: min(a.StreamWindowBytes, b.StreamWindowBytes),
		MaxStreams:        min(a.MaxStreams, b.MaxStreams), MaxRequests: min(a.MaxRequests, b.MaxRequests),
	}
}

// ActionSpec is the wire form of a declared action.
type ActionSpec struct {
	ID      string   `json:"id"`
	Title   string   `json:"title"`
	Accepts []string `json:"accepts"`
}

// Spec is the wire form of a plugin description.
type Spec struct {
	ID            string       `json:"id"`
	Version       string       `json:"version"`
	APIVersion    int          `json:"api_version"`
	Capabilities  []string     `json:"capabilities"`
	TransportKind string       `json:"transport_kind,omitempty"`
	// ExternalChannels and RemoteRevocation describe an external-channel sync
	// backend (decision 0009).
	ExternalChannels bool         `json:"external_channels,omitempty"`
	RemoteRevocation bool         `json:"remote_revocation,omitempty"`
	Actions       []ActionSpec `json:"actions,omitempty"`
}

// Equal compares ID, version, API, capabilities (as a set) and actions.
func (s Spec) Equal(o Spec) bool {
	if s.ID != o.ID || s.Version != o.Version || s.APIVersion != o.APIVersion || s.TransportKind != o.TransportKind || s.ExternalChannels != o.ExternalChannels || s.RemoteRevocation != o.RemoteRevocation {
		return false
	}
	norm := func(c []string) []string { return slices.Sorted(slices.Values(c)) }
	if !slices.Equal(norm(s.Capabilities), norm(o.Capabilities)) || len(s.Actions) != len(o.Actions) {
		return false
	}
	for i, a := range s.Actions {
		b := o.Actions[i]
		if a.ID != b.ID || a.Title != b.Title || !slices.Equal(a.Accepts, b.Accepts) {
			return false
		}
	}
	return true
}

type Hello struct {
	Protocol     int    `json:"protocol"`
	Nonce        string `json:"nonce"`
	ExpectedSpec Spec   `json:"expected_spec"`
	Limits       Limits `json:"limits"`
}

type HelloAck struct {
	Protocol int    `json:"protocol"`
	Nonce    string `json:"nonce"`
	Spec     Spec   `json:"spec"`
	Limits   Limits `json:"limits"`
}

// Method names.
const (
	MethodInit      = "plugin.init"
	MethodStart     = "plugin.start"
	MethodClose     = "plugin.close"
	MethodProcess   = "plugin.process"
	MethodRunAction = "plugin.run_action"
	MethodOnEvent   = "plugin.on_event"
	MethodNotify    = "host.notify"

	// External-channel sync backend profile (sync-backend-contract §6a).
	MethodSubmit       = "backend.submit"        // Engine→plugin
	MethodLeaveChannel = "backend.leave_channel" // Engine→plugin
	MethodRegister     = "backend.register"      // plugin→Engine
	MethodReport       = "backend.report"        // plugin→Engine
	MethodReceive      = "backend.receive"       // plugin→Engine
	MethodStream    = "stream.open"
)

const (
	MinTimeoutMs = 1
	MaxTimeoutMs = 300000
	MaxNesting   = 4
)

type Request struct {
	Method    string          `json:"method"`
	TimeoutMs U64             `json:"timeout_ms"`
	Parent    U64             `json:"parent_request_id"`
	Params    json.RawMessage `json:"params"`
}

type Response struct {
	OK     bool            `json:"ok"`
	Result json.RawMessage `json:"result,omitempty"`
	Error  *ErrorBody      `json:"error,omitempty"`
}

func (r Response) validate() error {
	if r.OK && (r.Error != nil || len(r.Result) == 0) {
		return protoErr("malformed success response")
	}
	if !r.OK && (r.Error == nil || len(r.Result) != 0) {
		return protoErr("malformed error response")
	}
	return nil
}

// Stable error codes; the wire never carries free text.
const (
	CodeInvalidArgument = "invalid_argument"
	CodeInvalidConfig   = "invalid_config"
	CodeInvalidState    = "invalid_state"
	CodeUnsupported     = "unsupported"
	CodePermission      = "permission_denied"
	CodeNotFound        = "not_found"
	CodeLimitExceeded   = "limit_exceeded"
	CodeExhausted       = "resource_exhausted"
	CodeUnavailable     = "unavailable"
	CodeTimeout         = "timeout"
	CodeCancelled       = "cancelled"
	CodePanic           = "plugin_panic"
	CodeInternal        = "internal"
)

var knownCodes = []string{"invalid_argument", "invalid_config", "invalid_state", "unsupported", "permission_denied", "not_found",
	"resource_expired", "conflict", "limit_exceeded", "resource_exhausted", "unavailable", "timeout", "cancelled", "plugin_panic",
	"internal", "identity_mismatch", "protocol_mismatch", "sync_disabled", "suspended", "no_targets", "source_changed"}

// ErrorBody is the wire error.
type ErrorBody struct {
	Code         string `json:"code"`
	Retryable    bool   `json:"retryable"`
	RetryAfterMs U64    `json:"retry_after_ms"`
}

// Error is the error returned by calls and handlers. Its text is only the
// stable code.
type Error struct {
	Code         string
	Retryable    bool
	RetryAfterMs uint64
}

func (e *Error) Error() string { return "plugin rpc: " + e.Code }

// NewError returns an error carrying only a stable code.
func NewError(code string) *Error { return &Error{Code: code} }

func (e *Error) body() *ErrorBody {
	code := e.Code
	if !slices.Contains(knownCodes, code) {
		code = CodeInternal
	}
	return &ErrorBody{Code: code, Retryable: e.Retryable, RetryAfterMs: U64(e.RetryAfterMs)}
}

func (b ErrorBody) err() *Error {
	return &Error{Code: b.Code, Retryable: b.Retryable, RetryAfterMs: uint64(b.RetryAfterMs)}
}

// Stream purposes valid for ordinary plugins.
const (
	PurposeProcessText = "process_text"
	PurposeActionText  = "action_text"
	// Sync backends only.
	PurposeSyncPart    = "sync_part"    // Engine→plugin, content to send
	PurposeReceivePart = "receive_part" // plugin→Engine, content that arrived
)

// Limits of the external-channel profile.
const (
	MaxSyncParts    = 8
	MaxSyncBytes    = 16 << 20 // total in-memory bytes per submit or receive
	MaxSyncTargets  = 1024
	MaxIDBytes      = 128
	MaxReceiveParts = 8
)

type SubmitPart struct {
	MIME   string     `json:"mime"`
	Size   U64        `json:"size"`
	Stream StreamDesc `json:"stream"`
}

// Submit asks the backend to send Parts to Targets (enabled channel IDs).
type Submit struct {
	TransferID  string       `json:"transfer_id"`
	TimestampMs U64          `json:"timestamp_ms"`
	Targets     []string     `json:"targets"`
	Parts       []SubmitPart `json:"parts"`
}

type SubmitResult struct {
	Queued bool `json:"queued"`
}

type LeaveChannel struct {
	ChannelID string `json:"channel_id"`
}

type Register struct {
	TransferID string   `json:"transfer_id"`
	Targets    []string `json:"targets"`
}

// Report states: queued, sending, sent, superseded, failed. Cause is dropped
// by the Engine; only the state is kept.
type Report struct {
	TransferID string `json:"transfer_id"`
	Target     string `json:"target"`
	State      string `json:"state"`
}

type ReceivePart struct {
	MIME   string     `json:"mime"`
	Stream StreamDesc `json:"stream"`
}

type Receive struct {
	ChannelID string        `json:"channel_id"`
	Transfer  string        `json:"transfer"`
	View      string        `json:"view"`
	Parts     []ReceivePart `json:"parts"`
}

// StreamDesc describes a binary stream; it never carries a path.
type StreamDesc struct {
	StreamID U64    `json:"stream_id"`
	Size     U64    `json:"size"`
	Purpose  string `json:"purpose"`
}

type Window struct {
	CreditBytes U64 `json:"credit_bytes"`
}

type End struct {
	OK   bool `json:"ok"`
	Size U64  `json:"size"`
}

type Empty struct{}

type Init struct {
	Config map[string]any `json:"config"`
	Events []string       `json:"events"`
}

type Notify struct {
	Message string `json:"message"`
}

type Origin struct {
	Kind     string `json:"kind"`
	DeviceID string `json:"device_id"`
	PluginID string `json:"plugin_id"`
}

type File struct {
	Name string `json:"name"`
	Size U64    `json:"size"`
}

type Item struct {
	ID        string      `json:"id"`
	Type      string      `json:"type"`
	Text      *StreamDesc `json:"text"`
	Formats   []string    `json:"formats"`
	Origin    Origin      `json:"origin"`
	SourceApp string      `json:"source_app"`
	Files     []File      `json:"files"`
}

type Policy struct {
	Persist     bool     `json:"persist"`
	Sync        bool     `json:"sync"`
	ExpiresAtMs U64      `json:"expires_at_ms"`
	Tags        []string `json:"tags"`
}

type Process struct {
	Event      string `json:"event"`
	Generation U64    `json:"generation"`
	Item       Item   `json:"item"`
	Policy     Policy `json:"policy"`
}

// Patch is a processor result. Absent fields keep the current value; it
// cannot carry content.
type Patch struct {
	Generation    U64       `json:"generation"`
	Persist       *bool     `json:"persist,omitempty"`
	Sync          *bool     `json:"sync,omitempty"`
	ExpireAfterMs *U64      `json:"expire_after_ms,omitempty"`
	Tags          *[]string `json:"tags,omitempty"`
}

type ActionRun struct {
	ActionID   string `json:"action_id"`
	Generation U64    `json:"generation"`
	Item       Item   `json:"item"`
}

type ActionResult struct {
	Generation U64        `json:"generation"`
	Text       StreamDesc `json:"text"`
}

const (
	MaxTags      = 32
	MaxTagBytes  = 64
	MaxActionLen = 64 << 20
)

func validUTF8(s string, max int) bool { return len(s) <= max && utf8.ValidString(s) }

func (d StreamDesc) validate(purpose string) error {
	if d.StreamID == 0 || d.Purpose != purpose {
		return protoErr("bad stream description")
	}
	return nil
}

// Validate checks semantic rules a JSON Schema cannot express.
func (p Process) Validate() error {
	if p.Event != "clipboard.captured" && p.Event != "clipboard.received" {
		return protoErr("bad event")
	}
	if err := p.Item.Validate(); err != nil {
		return err
	}
	return p.Policy.validate()
}

func (p Policy) validate() error {
	if len(p.Tags) > MaxTags {
		return protoErr("too many tags")
	}
	for _, t := range p.Tags {
		if t == "" || !validUTF8(t, MaxTagBytes) {
			return protoErr("bad tag")
		}
	}
	return nil
}

// Validate checks an item snapshot.
func (i Item) Validate() error {
	if i.ID == "" || len(i.ID) > 128 {
		return protoErr("bad item id")
	}
	switch i.Type {
	case "text", "image", "files":
	default:
		return protoErr("bad item type")
	}
	switch i.Origin.Kind {
	case "local_system", "remote_device", "action", "explicit":
	default:
		return protoErr("bad origin")
	}
	if i.Text != nil {
		if err := i.Text.validate(PurposeProcessText); err != nil {
			return err
		}
	}
	if len(i.Formats) > 4096 || len(i.Files) > 4096 {
		return protoErr("too many entries")
	}
	return nil
}

// Validate checks a patch before the engine applies it.
func (p Patch) Validate() error {
	if p.ExpireAfterMs != nil && (*p.ExpireAfterMs == 0 || *p.ExpireAfterMs > U64(1<<63-1)/1_000_000) {
		return NewError(CodeInvalidArgument)
	}
	if p.Tags != nil {
		if err := (Policy{Tags: *p.Tags}).validate(); err != nil {
			return NewError(CodeInvalidArgument)
		}
		seen := map[string]bool{}
		for _, t := range *p.Tags {
			if seen[t] {
				return NewError(CodeInvalidArgument)
			}
			seen[t] = true
		}
	}
	return nil
}

var actionIDRe = regexp.MustCompile(`^[a-z][a-z0-9_-]*$`)

func (r ActionRun) Validate() error {
	if len(r.ActionID) > 64 || !actionIDRe.MatchString(r.ActionID) {
		return protoErr("bad action id")
	}
	return r.Item.Validate()
}

func (w Window) validate() error {
	if w.CreditBytes == 0 {
		return protoErr("zero credit")
	}
	return nil
}

// Event is the payload of plugin.on_event: a read-only notification.
type Event struct {
	Kind   string `json:"kind"`
	ItemID string `json:"item_id"`
	AtMs   U64    `json:"at_ms"`
}

// Validate checks bounds only; the receiving side decides which kinds it knows.
func (e Event) Validate() error {
	if e.Kind == "" || len(e.Kind) > 128 || len(e.ItemID) > 128 {
		return protoErr("bad event")
	}
	return nil
}
