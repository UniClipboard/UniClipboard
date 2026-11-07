package engine

import "time"

// Public history DTOs (history-contract.md, sdk-extension-contract.md §3).
// They describe persisted records and never carry content, file names or paths.

type HistoryKind string
type HistoryOriginKind string
type HistorySyncState string

const (
	HistoryText  HistoryKind = "text"
	HistoryImage HistoryKind = "image"
	HistoryFiles HistoryKind = "files"

	OriginKindLocalSystem  HistoryOriginKind = "local_system"
	OriginKindRemoteDevice HistoryOriginKind = "remote_device"
	OriginKindPlugin       HistoryOriginKind = "plugin"
	OriginKindExplicit     HistoryOriginKind = "explicit"

	HistorySyncNotRequested HistorySyncState = "not_requested"
	HistorySyncPending      HistorySyncState = "pending"
	HistorySyncSent         HistorySyncState = "sent"
	HistorySyncPartial      HistorySyncState = "partial"
	HistorySyncFailed       HistorySyncState = "failed"
	HistorySyncInterrupted  HistorySyncState = "interrupted"
)

type HistoryOrigin struct {
	Kind     HistoryOriginKind `json:"kind"`
	DeviceID DeviceID          `json:"device_id,omitempty"`
	PluginID PluginID          `json:"plugin_id,omitempty"`
}

type HistoryPartSummary struct {
	Kind PartKind `json:"kind"`
	MIME string   `json:"mime,omitempty"`
	Size int64    `json:"size"`
}

type HistoryItem struct {
	ID              HistoryID            `json:"id"`
	Revision        uint64               `json:"revision,string"`
	Kind            HistoryKind          `json:"kind"`
	Origin          HistoryOrigin        `json:"origin"`
	SourceApp       string               `json:"source_app,omitempty"`
	AutomationDepth uint32               `json:"automation_depth"`
	CreatedAtMs     int64                `json:"created_at_ms"`
	UpdatedAtMs     int64                `json:"updated_at_ms"`
	ExpiresAtMs     int64                `json:"expires_at_ms"`
	Tags            []string             `json:"tags"`
	Parts           []HistoryPartSummary `json:"parts"`
	TotalBytes      int64                `json:"total_bytes"`
	SyncState       HistorySyncState     `json:"sync_state"`
}

type HistoryFilter struct {
	Kinds           []HistoryKind
	Tags            []string
	OriginKinds     []HistoryOriginKind
	DeviceID        DeviceID
	CreatedFromMs   int64
	CreatedBeforeMs int64
}

type ListHistoryRequest struct {
	Limit  int // 0: 50; 1..200
	Cursor string
	Filter HistoryFilter
}

type HistoryPage struct {
	Items           []HistoryItem `json:"items"`
	NextCursor      string        `json:"next_cursor,omitempty"`
	HistoryRevision uint64        `json:"history_revision,string"`
}

type UpdateItemRequest struct {
	ID               HistoryID
	ExpectedRevision uint64
	Tags             *[]string
	ExpiresAtMs      *int64
}

type DeleteItemRequest struct {
	ID               HistoryID
	ExpectedRevision uint64
}

// HistoryOptions is fixed at New. Zero fields take the defaults.
type HistoryOptions struct {
	MaxItems   int           // default 10000
	MaxBytes   int64         // default 4 GiB; at most ResourcePolicy.MaxStoredBytes
	DefaultTTL time.Duration // 0: items do not expire unless a processor says so
}
