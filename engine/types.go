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

// SyncSettings is the one global switch for content sync. Enabled=false keeps
// connections but neither sends nor applies content. What each backend syncs
// is Settings.BackendPolicies.
type SyncSettings struct {
	Enabled bool `json:"enabled"`
}

// BackendSyncPolicy is the standard per-backend policy Engine enforces for
// both sending and receiving. Text includes HTML and other non-image
// formats. AutoFileMaxBytes limits automatically sent files only: if any file
// is larger, that backend skips the whole automatic commit; 0 means no
// extra threshold. Manual (explicit) sends ignore it.
type BackendSyncPolicy struct {
	Text             bool  `json:"text"`
	Images           bool  `json:"images"`
	Files            bool  `json:"files"`
	AutoFileMaxBytes int64 `json:"auto_file_max_bytes"`
}

// DefaultBackendPolicy allows everything.
func DefaultBackendPolicy() BackendSyncPolicy {
	return BackendSyncPolicy{Text: true, Images: true, Files: true}
}

// Settings is the complete typed configuration. Parsing files and watching
// them is the host's job.
type Settings struct {
	DeviceName    string       `json:"device_name"` // empty: the default name chosen at first start
	RendezvousURL string       `json:"rendezvous_url"`
	Limits        Limits       `json:"limits"`
	Sync          SyncSettings `json:"sync"`
	// BackendPolicies has one entry per assembled sync backend. Missing
	// entries take DefaultBackendPolicy; an unknown ID is invalid_argument.
	// Reads return the complete effective map.
	BackendPolicies map[PluginID]BackendSyncPolicy `json:"backend_policies"`
}

func (s Settings) clone() Settings {
	m := make(map[PluginID]BackendSyncPolicy, len(s.BackendPolicies))
	for k, v := range s.BackendPolicies {
		m[k] = v
	}
	s.BackendPolicies = m
	return s
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

// SensitiveTicket is a full pairing ticket as text. Whoever holds it can
// start pairing with this device (which still needs the user's approval on
// both sides); treat it like a password.
type SensitiveTicket struct {
	Ticket      string `json:"ticket"`
	ExpiresAtMs int64  `json:"expires_at_ms"`
}

// JoinRequest pairs with the device showing Code.
type JoinRequest struct {
	RequestID RequestID
	Code      string
}

// JoinTicketRequest pairs with the device that issued Ticket.
type JoinTicketRequest struct {
	RequestID RequestID
	Ticket    string
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

// PairingResult is the outcome of a successful pairing operation. The device
// is now in the device list as pending: the user compares Fingerprint with the
// other screen, then calls TrustDevice and AuthorizeBinding. Waiting for the
// other side's approval is a success; the path connects once both agree.
type PairingResult struct {
	PeerID                 DeviceID `json:"peer_id"`
	Fingerprint            string   `json:"fingerprint"`
	AwaitingLocalApproval  bool     `json:"awaiting_local_approval"`
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
	// BackendIDs, if non-empty, limits the send to those backends (sorted,
	// validated by CommitContent).
	BackendIDs []PluginID

	action *actionRun // set by RunAction only
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
	BackendID  PluginID       `json:"backend_id,omitempty"`
	TargetKind SyncTargetKind `json:"target_kind,omitempty"`
	TargetID   string         `json:"target_id,omitempty"` // opaque; for a device the DeviceID
	PeerID     DeviceID       `json:"peer_id,omitempty"`   // devices only, kept for older clients
	State      TargetState    `json:"state"`
	Error      *Error         `json:"error,omitempty"`
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
	InstanceID   InstanceID        `json:"instance_id"`
	Revision     uint64            `json:"revision,string"`
	Lifecycle    Lifecycle         `json:"lifecycle"`
	Runtime      RuntimeState      `json:"runtime"`
	Self         Self              `json:"self"`
	Settings     SettingsState     `json:"settings"`
	Capabilities Capabilities      `json:"capabilities"`
	Devices      []Device          `json:"devices"`
	PairingCode  PairingCodeState  `json:"pairing_code"`
	Operations   []Operation       `json:"operations"`
	Received     []ReceivedContent `json:"received"`
	// HistoryRevision is the persisted revision of the history collection.
	HistoryRevision uint64 `json:"history_revision,string"`
	// ActiveApply is the application in progress, if any. Only one at a time.
	ActiveApply *ActiveApply `json:"active_apply,omitempty"`
}

// ApplyKind says what an ActiveApply writes.
type ApplyKind string

const (
	ApplyKindReceived ApplyKind = "received"
	ApplyKindHistory  ApplyKind = "history"
)

// ActiveApply names the received content a host is writing to the system.
type ActiveApply struct {
	Kind      ApplyKind `json:"kind"`
	HistoryID HistoryID `json:"history_id,omitempty"` // only for Kind history; may be deleted meanwhile
	ReceiptID ReceiptID `json:"receipt_id"`
	ContentID ContentID `json:"content_id"`
}

// ReceiptID names an entry of the received list.
type ReceiptID string

// ApplyState is whether this device has put received content to use.
type ApplyState string

const (
	ApplyAvailable ApplyState = "available" // received and readable, not yet applied
	ApplyApplying  ApplyState = "applying"
	ApplyApplied   ApplyState = "applied"
	ApplyFailed    ApplyState = "failed"
	ApplyDeferred  ApplyState = "deferred"
)

// ReceivedContent is content that arrived from a paired device. It is listed
// for a limited time and count; it is not a history and does not survive a
// restart. Read it through RetainContent and ReadContent.
type ReceivedContent struct {
	ReceiptID    ReceiptID         `json:"receipt_id"`
	Content      ContentDescriptor `json:"content"`
	PeerID       DeviceID          `json:"peer_id"`
	ReceivedAtMs int64             `json:"received_at_ms"`
	ExpiresAtMs  int64             `json:"expires_at_ms"`
	ApplyState   ApplyState        `json:"apply_state"`
	// HistoryID links the persisted record; empty when persist was off. The
	// record may be gone already, so GetItem can return not_found.
	HistoryID HistoryID `json:"history_id,omitempty"`
}

// EventKind says which part of State changed.
type EventKind string

const (
	EventLifecycle    EventKind = "lifecycle"
	EventRuntime      EventKind = "runtime"
	EventSettings     EventKind = "settings"
	EventCapabilities EventKind = "capabilities"
	EventPairingCode  EventKind = "pairing_code"
	EventOperations   EventKind = "operations"
	EventReceived     EventKind = "received"
	EventApply        EventKind = "apply"
	EventHistory      EventKind = "history" // Objects: type "history", ID = HistoryID
	EventDevices      EventKind = "devices" // Objects: type "device", ID = Device.ID
	// EventResourceWarning is a hint that something was refused for lack of
	// room. It is not a task: losing it loses nothing.
	EventResourceWarning EventKind = "resource_warning"
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

// ItemID names one processed item inside this Engine instance. It differs
// from ContentID and from the future persistent HistoryID.
type ItemID string

// HistoryID is reserved for persistent history; it stays empty until then.
type HistoryID string

type SubmissionPhase string

const (
	SubmissionProcessing SubmissionPhase = "processing"
	SubmissionCommitted  SubmissionPhase = "committed"
	SubmissionFailed     SubmissionPhase = "failed" // failed before commit; nothing was stored or sent
)

// SyncDisposition is the sync decision taken at commit.
type SyncDisposition string

const (
	SyncNotRequested SyncDisposition = "not_requested" // processing set sync=false
	SyncDisabled     SyncDisposition = "disabled"      // sync is off globally
	SyncPolicyFilter SyncDisposition = "policy_filtered"
	SyncNoTargets    SyncDisposition = "no_targets"
	SyncNoBackend    SyncDisposition = "no_backend" // no sync backend is enabled
	SyncQueued       SyncDisposition = "queued"
	SyncFailed       SyncDisposition = "failed"
)

// CommitRequest is an explicit submission. BackendIDs and WritebackGuard are
// reserved: non-nil values are rejected until multiple backends and automatic
// writeback exist.
type CommitRequest struct {
	RequestID      RequestID
	LeaseID        LeaseID
	WritebackGuard *WritebackGuard
	BackendIDs     []PluginID
}

type PluginID string

// WritebackGuard names the observation a future automatic writeback is bound to.
type WritebackGuard struct {
	Epoch    string
	Sequence uint64
}

// SubmissionResult is a snapshot without content. Phase is committed only
// when processing finished and the result was accepted; whether anything was
// sent is SyncDisposition's business, not Error's.
type SubmissionResult struct {
	RequestID       RequestID       `json:"request_id"`
	Phase           SubmissionPhase `json:"phase"`
	ItemID          ItemID          `json:"item_id"`
	HistoryID       HistoryID       `json:"history_id,omitempty"`
	SyncDisposition SyncDisposition `json:"sync_disposition,omitempty"`
	OperationID     OperationID     `json:"operation_id,omitempty"`
	// BackendResults has one entry per backend the commit was offered to,
	// sorted by ID. A queued aggregate does not mean every backend queued.
	BackendResults []BackendSubmissionResult `json:"backend_results,omitempty"`
	SyncError      *Error                    `json:"sync_error,omitempty"`
	Error          *Error                    `json:"error,omitempty"`
}
