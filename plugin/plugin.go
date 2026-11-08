// Package plugin is the public contract between the engine and its plugins.
// It does not import the engine; the engine adapts these types. See
// docs/design/plugin-contract.md.
package plugin

import (
	"context"
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
	Type    string // text, image or files
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
