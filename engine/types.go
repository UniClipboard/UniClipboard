package engine

// Identifiers are opaque, case-sensitive strings. Never derive anything from
// their content.
type (
	// DeviceID is stable across processes and restarts.
	DeviceID string
	// InstanceID changes with every New.
	InstanceID string
	// OperationID names an asynchronous operation of one instance.
	OperationID string
	// RequestID is chosen by the caller to make a request idempotent.
	RequestID string
)

// Lifecycle is where an instance is in its life.
type Lifecycle string

const (
	LifecycleCreated  Lifecycle = "created"
	LifecycleStarting Lifecycle = "starting"
	LifecycleRunning  Lifecycle = "running"
	LifecycleStopping Lifecycle = "stopping"
	LifecycleStopped  Lifecycle = "stopped"
	LifecycleClosed   Lifecycle = "closed"
)

// RuntimeState tells the engine whether the host lets it use the network.
type RuntimeState string

const (
	RuntimeActive    RuntimeState = "active"
	RuntimeSuspended RuntimeState = "suspended"
)

// ---- settings ----

// Limits bound what is sent and accepted. They cannot exceed the wire limits
// (2 GiB per file, 4 GiB per clipboard).
type Limits struct {
	MaxFileBytes  int64 `json:"max_file_bytes"`
	MaxTotalBytes int64 `json:"max_total_bytes"`
}

// SyncSettings chooses what is synchronized. Enabled=false keeps connections
// but neither sends nor applies content.
type SyncSettings struct {
	Enabled bool `json:"enabled"`
	Text    bool `json:"text"`
	Images  bool `json:"images"`
	Files   bool `json:"files"`
}

// Settings is the complete typed configuration. Parsing files and watching
// them is the host's job.
type Settings struct {
	DeviceName    string       `json:"device_name"` // empty: the default name chosen at first start
	RendezvousURL string       `json:"rendezvous_url"`
	Limits        Limits       `json:"limits"`
	Sync          SyncSettings `json:"sync"`
}

// SettingsState is the effective settings and their revision.
type SettingsState struct {
	Revision uint64   `json:"revision,string"`
	Value    Settings `json:"value"`
}

// UpdateSettingsRequest replaces the settings as a whole. ExpectedRevision
// must be the revision the caller based Value on.
type UpdateSettingsRequest struct {
	ExpectedRevision uint64
	Value            Settings
}

// ---- capabilities ----

// Ability is the availability of a host function.
type Ability string

const (
	AbilityAvailable          Ability = "available"
	AbilityPermissionRequired Ability = "permission_required"
	AbilityUnavailable        Ability = "unavailable"
	AbilityUnsupported        Ability = "unsupported"
)

// MIME types the core understands. Files are always supported.
const (
	MimeText = "text/plain"
	MimeHTML = "text/html"
	MimePNG  = "image/png"
)

// HostCapabilities is what the host can do right now. By default nothing.
type HostCapabilities struct {
	ReadFormats  []string `json:"read_formats"`  // MIME types the host can read from the clipboard
	WriteFormats []string `json:"write_formats"` // MIME types the host can write
	ReadFiles    bool     `json:"read_files"`
	WriteFiles   bool     `json:"write_files"`
	Observe      Ability  `json:"observe"` // watching the clipboard for changes
	Apply        Ability  `json:"apply"`   // writing received content
}

// Capabilities is what the engine will actually do: the host's declaration,
// the formats the core supports, and their intersection.
type Capabilities struct {
	Host        HostCapabilities `json:"host"`
	CoreFormats []string         `json:"core_formats"`
	Effective   HostCapabilities `json:"effective"`
	// Restrictions explains why something is unavailable though the host could
	// do it: "not_running", "suspended", "sync_disabled".
	Restrictions []string `json:"restrictions,omitempty"`
}

// ---- pairing and devices ----

// Connection is the link to a paired device.
type Connection string

const (
	Disconnected Connection = "disconnected"
	Connecting   Connection = "connecting"
	Connected    Connection = "connected"
)

// Peer is a paired device. Paired does not mean connected.
type Peer struct {
	ID            DeviceID   `json:"id"`
	Name          string     `json:"name"`
	TrustRevision uint64     `json:"trust_revision,string"`
	Connection    Connection `json:"connection"`
	LastSeenMs    int64      `json:"last_seen_ms,omitempty"`
}

// PendingSource tells how a device came to ask for approval.
type PendingSource string

const (
	SourceDirect       PendingSource = "direct"
	SourceCode         PendingSource = "code"
	SourceIntroduction PendingSource = "introduction"
)

// PendingPeer is a device waiting for the user's decision.
type PendingPeer struct {
	ID              DeviceID      `json:"id"`
	Name            string        `json:"name"`
	RequestRevision uint64        `json:"request_revision,string"`
	Source          PendingSource `json:"source"`
	// IntroducedByName is the name of the paired device that introduced this
	// one (SourceIntroduction only). Names are not identities.
	IntroducedByName string `json:"introduced_by_name,omitempty"`
	// Fingerprint is the same on both devices; the user compares it before
	// approving.
	Fingerprint string `json:"fingerprint"`
	SeenAtMs    int64  `json:"seen_at_ms"`
}

// PairingCode is a code this device shows. It is sensitive: only the explicit
// methods return it, never State or events.
type PairingCode struct {
	Code        string `json:"code"` // "NNN-NNN"
	ExpiresAtMs int64  `json:"expires_at_ms"`
}

// PairingCodeState says whether a code is showing, without revealing it.
type PairingCodeState struct {
	Active      bool  `json:"active"`
	ExpiresAtMs int64 `json:"expires_at_ms,omitempty"`
}

// SensitiveAddress is this device's address. Anyone holding it can connect to
// this device and ask for approval; treat it like a password.
type SensitiveAddress struct {
	Address string `json:"address"`
}

// JoinRequest pairs with the device showing Code.
type JoinRequest struct {
	RequestID RequestID
	Code      string
}

// PairRequest pairs with the device at Address.
type PairRequest struct {
	RequestID RequestID
	Address   string
}

// ApprovalRequest approves exactly the request the user looked at.
type ApprovalRequest struct {
	DeviceID            DeviceID
	RequestRevision     uint64
	ExpectedFingerprint string
}

// PendingRequest names a pending request, for Reject.
type PendingRequest struct {
	DeviceID        DeviceID
	RequestRevision uint64
}

// ---- operations ----

// OperationKind is what an operation does.
type OperationKind string

const (
	OperationPairing OperationKind = "pairing"
	OperationSend    OperationKind = "send"
)

// Phase is how far an operation is.
type Phase string

const (
	PhaseResolving   Phase = "resolving"
	PhaseConnecting  Phase = "connecting"
	PhaseHandshaking Phase = "handshaking"
	PhaseSucceeded   Phase = "succeeded"
	PhaseFailed      Phase = "failed"
	PhaseQueued      Phase = "queued"
	PhaseSending     Phase = "sending"
	PhaseCompleted   Phase = "completed"
)

// PairingResult is the outcome of a successful pairing operation. Waiting for
// the other device's approval is a success: the device is remembered and
// connects as soon as it approves.
type PairingResult struct {
	PeerID                 DeviceID `json:"peer_id"`
	Fingerprint            string   `json:"fingerprint"`
	AwaitingRemoteApproval bool     `json:"awaiting_remote_approval"`
}

// Operation tracks an asynchronous request. It never contains the code or the
// address that started it.
type Operation struct {
	ID          OperationID    `json:"id"`
	RequestID   RequestID      `json:"request_id"`
	Kind        OperationKind  `json:"kind"`
	CreatedAtMs int64          `json:"created_at_ms"`
	UpdatedAtMs int64          `json:"updated_at_ms"`
	Phase       Phase          `json:"phase"`
	Pairing     *PairingResult `json:"pairing,omitempty"`
	Error       *Error         `json:"error,omitempty"`
	// Send operations only:
	Targets []TargetStatus `json:"targets,omitempty"`
	Outcome Outcome        `json:"outcome,omitempty"`
}

// Done reports whether the operation reached a final phase.
func (o Operation) Done() bool {
	return o.Phase == PhaseSucceeded || o.Phase == PhaseFailed || o.Phase == PhaseCompleted
}

// Origin says where submitted content came from.
type Origin string

const (
	OriginExplicit  Origin = "explicit"  // the user asked to send it
	OriginClipboard Origin = "clipboard" // seen by ObserveClipboard; not accepted from outside
)

// SubmitRequest asks the engine to send leased content to the devices that are
// connected right now.
type SubmitRequest struct {
	RequestID RequestID
	LeaseID   LeaseID
	Origin    Origin
}

// TargetState is how far content got towards one device.
type TargetState string

const (
	TargetQueued     TargetState = "queued"
	TargetSending    TargetState = "sending"
	TargetSent       TargetState = "sent" // every byte was written to the connection; says nothing about the other side
	TargetFailed     TargetState = "failed"
	TargetSuperseded TargetState = "superseded" // a newer submission replaced it before it started
)

func (t TargetState) final() bool {
	return t == TargetSent || t == TargetFailed || t == TargetSuperseded
}

// TargetStatus is the progress towards one device.
type TargetStatus struct {
	PeerID DeviceID    `json:"peer_id"`
	State  TargetState `json:"state"`
	Error  *Error      `json:"error,omitempty"`
}

// Outcome summarizes a finished send.
type Outcome string

const (
	OutcomeSuccess    Outcome = "success"    // every target is sent
	OutcomePartial    Outcome = "partial"    // some sent, some not
	OutcomeFailed     Outcome = "failed"     // none sent
	OutcomeSuperseded Outcome = "superseded" // every target was replaced by newer content
)

// ---- state and events ----

// Self identifies this device.
type Self struct {
	ID   DeviceID `json:"id"`
	Name string   `json:"name"`
}

// State is a consistent, secret-free snapshot.
type State struct {
	InstanceID   InstanceID       `json:"instance_id"`
	Revision     uint64           `json:"revision,string"`
	Lifecycle    Lifecycle        `json:"lifecycle"`
	Runtime      RuntimeState     `json:"runtime"`
	Self         Self             `json:"self"`
	Settings     SettingsState    `json:"settings"`
	Capabilities Capabilities     `json:"capabilities"`
	Peers        []Peer           `json:"peers"`
	Pending      []PendingPeer    `json:"pending"`
	PairingCode  PairingCodeState `json:"pairing_code"`
	Operations   []Operation      `json:"operations"`
}

// EventKind says which part of State changed.
type EventKind string

const (
	EventLifecycle    EventKind = "lifecycle"
	EventRuntime      EventKind = "runtime"
	EventSettings     EventKind = "settings"
	EventCapabilities EventKind = "capabilities"
	EventPeers        EventKind = "peers"
	EventPending      EventKind = "pending"
	EventPairingCode  EventKind = "pairing_code"
	EventOperations   EventKind = "operations"
)

// ObjectRef points at something that changed; fetch details with a query.
type ObjectRef struct {
	Type string `json:"type"` // "operation"
	ID   string `json:"id"`
}

// Event announces one change. It carries no content and no secrets. Events are
// hints: the State is the truth, so a consumer that misses one just re-reads.
type Event struct {
	InstanceID InstanceID  `json:"instance_id"`
	Revision   uint64      `json:"revision,string"`
	AtMs       int64       `json:"at_ms"`
	Kind       EventKind   `json:"kind"`
	Objects    []ObjectRef `json:"objects,omitempty"`
}
