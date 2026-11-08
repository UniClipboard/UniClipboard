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
)

// Events a processor can subscribe to.
const (
	EventCaptured = "clipboard.captured"
	EventReceived = "clipboard.received"
)

// Spec identifies a plugin and what it offers.
type Spec struct {
	ID           string // e.g. "builtin.text"
	Version      string
	APIVersion   int // 1
	Capabilities []string
	Actions      []ActionSpec // only with CapAction
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
