package api

import (
	"context"

	"github.com/uniclipboard/unic/engine"
)

type HistorySearchHit struct {
	Item    HistoryItem                 `json:"item"`
	Preview engine.HistorySearchPreview `json:"preview"`
}
type HistorySearchPage struct {
	Hits            []HistorySearchHit `json:"hits"`
	NextCursor      string             `json:"next_cursor,omitempty"`
	HistoryRevision uint64             `json:"history_revision,string"`
	EvaluatedAtMs   int64              `json:"evaluated_at_ms"`
	IndexIncomplete bool               `json:"index_incomplete,omitempty"`
}
type HistorySearchBackend interface {
	SearchHistory(context.Context, engine.HistorySearchRequest) (HistorySearchPage, error)
	ListHistorySearchCandidates(context.Context, engine.HistorySearchCandidatesRequest) (engine.HistorySearchCandidatesPage, error)
	CountHistorySearches(context.Context, engine.HistorySearchCountsRequest) (engine.HistorySearchCounts, error)
}

func (c *Client) SearchHistory(ctx context.Context, q engine.HistorySearchRequest) (out HistorySearchPage, err error) {
	err = c.do(ctx, "POST", "/v1/history/search", q, &out)
	return
}
func (c *Client) ListHistorySearchCandidates(ctx context.Context, q engine.HistorySearchCandidatesRequest) (out engine.HistorySearchCandidatesPage, err error) {
	err = c.do(ctx, "POST", "/v1/history/search/candidates", q, &out)
	return
}
func (c *Client) CountHistorySearches(ctx context.Context, q engine.HistorySearchCountsRequest) (out engine.HistorySearchCounts, err error) {
	err = c.do(ctx, "POST", "/v1/history/search/counts", q, &out)
	return
}
