package engine

import "encoding/json"

// HistoryAnnotationAsset describes one binary attachment owned by an annotation.
type HistoryAnnotationAsset struct {
	Key  string `json:"key"`
	MIME string `json:"mime"`
	Size int64  `json:"size"`
}

// HistoryAnnotation is one persisted derived annotation for a history record.
// Annotations are local-only, ordered by configured plugin priority, and never
// included in ordinary ListHistory metadata responses.
type HistoryAnnotation struct {
	PluginID      PluginID                 `json:"plugin_id"`
	Type          string                   `json:"type"`
	PluginVersion string                   `json:"plugin_version"`
	Payload       json.RawMessage          `json:"payload"`
	Assets        []HistoryAnnotationAsset `json:"assets,omitempty"`
	UpdatedAtMs   int64                    `json:"updated_at_ms"`
}

// HistoryEnrichRequest requests high-priority on-demand or manual enrichment
// for an existing persisted history record.
type HistoryEnrichRequest struct {
	HistoryID HistoryID `json:"history_id"`
	PluginID  PluginID  `json:"plugin_id,omitempty"`
	Force     bool      `json:"force,omitempty"`
}
