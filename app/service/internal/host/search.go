package host

import (
	"context"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

func (n *Node) SearchHistory(ctx context.Context, q engine.HistorySearchRequest) (api.HistorySearchPage, error) {
	page, err := n.e.SearchHistory(ctx, q)
	if err != nil {
		return api.HistorySearchPage{}, apiErr0(err)
	}
	names := n.names(ctx)
	out := api.HistorySearchPage{Hits: []api.HistorySearchHit{}, NextCursor: page.NextCursor, HistoryRevision: page.HistoryRevision, EvaluatedAtMs: page.EvaluatedAtMs, IndexIncomplete: page.IndexIncomplete}
	for _, hit := range page.Hits {
		out.Hits = append(out.Hits, api.HistorySearchHit{Item: apiHistory(hit.Item, names[hit.Item.Origin.DeviceID]), Preview: hit.Preview})
	}
	return out, nil
}
func (n *Node) ListHistorySearchCandidates(ctx context.Context, q engine.HistorySearchCandidatesRequest) (engine.HistorySearchCandidatesPage, error) {
	v, e := n.e.ListHistorySearchCandidates(ctx, q)
	return v, apiErr0(e)
}
func (n *Node) CountHistorySearches(ctx context.Context, q engine.HistorySearchCountsRequest) (engine.HistorySearchCounts, error) {
	v, e := n.e.CountHistorySearches(ctx, q)
	return v, apiErr0(e)
}
