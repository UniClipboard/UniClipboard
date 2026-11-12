// Package plugin is the public contract between the engine and its plugins.
// It does not import the engine; the engine adapts these types. See
// docs/contracts/plugin-contract.md.
package plugin

import (
	"context"
	"io"
	"time"
)

// Capabilities a plugin may declare.
const (
	CapProcessor = "processor" // annotates clipboard items automatically
	CapAction    = "action"    // transforms content when the user asks
	CapObserver  = "observer"  // is told, read-only, about committed history changes
	// CapSyncBackend carries clipboard content between devices. Declaring it
	// needs a TransportKind; the engine loads such plugins from P5 on.
	CapSyncBackend = "sync_backend"
)

// Transport kinds of a sync backend.
const (
	TransportPeerSessions = "peer_sessions" // direct sessions with paired devices
	TransportObjectStore  = "object_store"  // a shared store such as WebDAV
)

// Events a processor can subscribe to.
const (
	EventCaptured = "clipboard.captured"
	EventReceived = "clipboard.received"
)

// Events an observer can subscribe to. They carry the history ID only, never
// content, and are hints: losing one loses nothing a query would not show.
const (
	EventCommitted   = "clipboard.committed" // an item entered the history
	EventUpdated     = "clipboard.updated"   // its tags or expiry changed
	EventDeleted     = "clipboard.deleted"   // the user deleted it, or it was evicted
	EventExpired     = "clipboard.expired"   // it reached its expiry
	EventPasted      = "clipboard.pasted"    // the user applied it through the host's UI; direct system pastes are not observable
	EventAppStarted  = "app.started"         // the engine is running; ItemID is empty
	EventAppShutdown = "app.shutdown"        // the engine is stopping; ItemID is empty
)

// Spec identifies a plugin and what it offers.
type Spec struct {
	ID           string // e.g. "builtin.text"
	Version      string
	APIVersion   int // 1
	Capabilities []string
	// TransportKind is for sync backends only; other plugins leave it empty.
	TransportKind string
	Actions       []ActionSpec // only with CapAction
	// ExternalChannels (sync backends only) says the backend's peers are
	// external channels (decision 0009), not unic devices: no key, no
	// fingerprint, authorized by being enabled. Its targets are channel IDs
	// the engine gives it in SyncView.Targets.
	ExternalChannels bool
	// RemoteRevocation (external-channel backends only) says the backend can
	// remove this device from the remote group when a channel is removed.
	RemoteRevocation bool
}

// ActionSpec declares one user-invoked action. Its key outside the plugin is
// Spec.ID + "/" + ID.
type ActionSpec struct {
	ID      string
	Title   string
	Accepts []string // "text" in this version
}

// Config holds plugin-specific settings as JSON values.
type Config map[string]any

// Host is what a plugin may ask of the application around it.
type Host interface {
	// Notify shows a short message to the user. It is rate limited and
	// dropped when nobody is listening.
	Notify(ctx context.Context, message string)
}

type Plugin interface {
	Spec() Spec
	Init(context.Context, Host, Config) error
	Start(context.Context) error
	Close(context.Context) error
}

// Processor annotates items as they are captured or received. It cannot
// change content.
type Processor interface {
	Plugin
	Process(context.Context, ProcessContext) error
}

// Event is one read-only notification for an observer.
type Event struct {
	Kind   string // one of the observer events
	ItemID string // history ID
	AtMs   int64
}

// Observer is told about history changes after they happened. Its errors are
// dropped; it can neither veto nor change anything.
type Observer interface {
	Plugin
	OnEvent(context.Context, Event) error
}

// Actor runs the actions it declared, only when a user asks.
type Actor interface {
	Plugin
	Run(context.Context, ActionContext) error
}

type Origin struct {
	Kind     string // local_system, remote_device, explicit or action
	DeviceID string
	PluginID string
}

type File struct {
	Name string
	Size int64
}

// Item is a read-only description of the content being processed.
type Item struct {
	ID      string
	Type    string // text, image, files, directory or directory_reference
	Text    string // empty unless the item has a text representation
	Formats []string
	Files   []File
	Origin  Origin
}

// Policy is what will happen to the item after processing.
type Policy struct {
	Persist     bool
	Sync        bool
	Tags        []string
	ExpireAfter time.Duration // zero: never
}

// ProcessContext shows the item and policy so far and stages changes to the
// policy. A failed call, or an error returned from Process, drops them all.
type ProcessContext interface {
	Event() string
	Item() Item
	Policy() Policy
	AddTag(string) error
	RemoveTag(string) error
	SetPersist(bool) error
	SetSync(bool) error
	SetExpire(time.Duration) error
}

// ActionContext gives an action its source item and takes its one result.
type ActionContext interface {
	Action() string // the ActionSpec.ID being run
	Item() Item
	// SetText supplies the result. Call it exactly once.
	SetText(string) error
}

// SyncState is how a target fares during one send.
type SyncState string

const (
	SyncQueued     SyncState = "queued"
	SyncSending    SyncState = "sending"
	SyncSent       SyncState = "sent"
	SyncSuperseded SyncState = "superseded" // replaced by newer content before it went out
	SyncFailed     SyncState = "failed"
)

// SyncPart is one part of the content to send.
type SyncPart struct {
	MIME string
	File bool   // a file part; Name is its display name
	Name string // files only
	Size int64
	// Open reads the part. Representations are small; files may be large and
	// can fail with a changed-source error.
	Open func(ctx context.Context) (io.ReadCloser, error)
}

// SyncView is the content one backend is asked to send, already filtered by
// that backend's policy.
type SyncView struct {
	ID          string // transfer identity, shared by all backends of a commit
	TimestampMs int64
	Parts       []SyncPart
	// Targets are the enabled channel IDs of an external-channel backend; nil
	// for other backends.
	Targets []string
}

// SyncHost reports the fate of a send. Targets are devices for peer_sessions
// backends and repositories for object_store ones; IDs are opaque and public.
type SyncHost interface {
	// Register announces the targets about to receive work, before any of them
	// starts.
	Register(targets []string) error
	// Report tells how a registered target fares. cause is never shown to the
	// user verbatim.
	Report(target string, state SyncState, cause error)
}

// SyncBackend is implemented by plugins that declare CapSyncBackend. Submit
// queues the view for the backend's targets without waiting for the network;
// queued=false with a nil error means there was nobody to send to.
type SyncBackend interface {
	Plugin
	Submit(ctx context.Context, v SyncView, h SyncHost) (queued bool, err error)
}

// ChannelLeaver is implemented by external-channel backends that declare
// RemoteRevocation: LeaveChannel removes this device from the remote group.
// The engine deletes its own channel record only after it returns nil.
type ChannelLeaver interface {
	LeaveChannel(ctx context.Context, channelID string) error
}

// ReceivedPart is one in-memory representation of received content.
type ReceivedPart struct {
	MIME string
	Data []byte
}

// Received is content that arrived from a channel. Transfer is stable per
// remote message and View per exact content; together they let the engine
// drop the same message arriving twice, also through another backend.
type Received struct {
	Transfer string
	View     string
	Parts    []ReceivedPart
}

// Receiver takes content that arrived from an external channel. A non-nil
// error means the engine declined it (disabled channel, policy, no room); the
// backend drops the content and must not retry it.
type Receiver interface {
	Receive(ctx context.Context, channelID string, r Received) error
}

// ReceiverAware is implemented by external-channel backends; the engine hands
// over the Receiver after Init and before Start.
type ReceiverAware interface {
	SetReceiver(Receiver)
}

// DirAware is implemented by in-process sync backends that keep state. The
// engine hands over the backend's private instance directory after Init and
// before Start; out-of-process backends are given theirs as the working
// directory instead.
type DirAware interface {
	SetDir(dir string)
}
