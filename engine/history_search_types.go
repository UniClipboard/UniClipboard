package engine

// Search is an explicit, bounded content read. Ordinary history DTOs remain metadata-only.
type HistorySearchType string
type HistoryTagMatch string
type HistorySearchSource struct {
	Kind     string   `json:"kind"`
	DeviceID DeviceID `json:"device_id,omitempty"`
}
type HistorySearchQuery struct {
	TextTerms       []string              `json:"text_terms,omitempty"`
	Types           []HistorySearchType   `json:"types,omitempty"`
	Tags            []string              `json:"tags,omitempty"`
	TagMatch        HistoryTagMatch       `json:"tag_match,omitempty"`
	Sources         []HistorySearchSource `json:"sources,omitempty"`
	Extensions      []string              `json:"extensions,omitempty"`
	CreatedFromMs   int64                 `json:"created_from_ms,omitempty"`
	CreatedBeforeMs int64                 `json:"created_before_ms,omitempty"`
}
type HistorySearchRequest struct {
	Query  HistorySearchQuery `json:"query"`
	Cursor string             `json:"cursor,omitempty"`
	Limit  int                `json:"limit,omitempty"`
}
type HistorySearchRange struct {
	Start int `json:"start"`
	End   int `json:"end"`
}
type HistorySearchPreview struct {
	Text       string               `json:"text"`
	Highlights []HistorySearchRange `json:"highlights"`
}
type HistorySearchHit struct {
	Item    HistoryItem          `json:"item"`
	Preview HistorySearchPreview `json:"preview"`
}
type HistorySearchPage struct {
	Hits            []HistorySearchHit `json:"hits"`
	NextCursor      string             `json:"next_cursor,omitempty"`
	HistoryRevision uint64             `json:"history_revision,string"`
	EvaluatedAtMs   int64              `json:"evaluated_at_ms"`
	IndexIncomplete bool               `json:"index_incomplete,omitempty"`
}
type HistorySearchCandidatesRequest struct {
	Field  string `json:"field"`
	Prefix string `json:"prefix,omitempty"`
	Cursor string `json:"cursor,omitempty"`
	Limit  int    `json:"limit,omitempty"`
}
type HistorySearchCandidate struct {
	Value  string               `json:"value"`
	Label  string               `json:"label"`
	Source *HistorySearchSource `json:"source,omitempty"`
}
type HistorySearchCandidatesPage struct {
	Candidates      []HistorySearchCandidate `json:"candidates"`
	NextCursor      string                   `json:"next_cursor,omitempty"`
	HistoryRevision uint64                   `json:"history_revision,string"`
	EvaluatedAtMs   int64                    `json:"evaluated_at_ms"`
}
type HistorySearchCountsRequest struct {
	Queries []HistorySearchQuery `json:"queries"`
}
type HistorySearchCounts struct {
	Counts          []uint64 `json:"counts"`
	HistoryRevision uint64   `json:"history_revision,string"`
	EvaluatedAtMs   int64    `json:"evaluated_at_ms"`
}
