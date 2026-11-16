package desktopui

import (
	"context"
	"fmt"
	"slices"
	"time"

	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
)

const historyWindowLimit = 500
const historyCheckpointLimit = 128
const historyRecoveryInterval = 30 * time.Second

type historyCheckpoint struct {
	offset int
	cursor string
}

// Checkpoints contain no content. Thin older checkpoints instead of allowing
// cursor memory to grow with the number of pages visited. A missing checkpoint
// costs sequential cursor reads, never an unbounded allocation.
func rememberHistoryCheckpoint(points []historyCheckpoint, p historyCheckpoint) []historyCheckpoint {
	at := slices.IndexFunc(points, func(v historyCheckpoint) bool { return v.offset >= p.offset })
	if at >= 0 && points[at].offset == p.offset {
		points[at] = p
		return points
	}
	if at < 0 {
		points = append(points, p)
	} else {
		points = slices.Insert(points, at, p)
	}
	if len(points) > historyCheckpointLimit {
		compact := []historyCheckpoint{points[0]}
		for i := 2; i < len(points)-1; i += 2 {
			compact = append(compact, points[i])
		}
		points = append(compact, points[len(points)-1])
	}
	return points
}

func (l *liveState) loadPrevious(d *viewState) {
	if l.windowOffset == 0 || l.loading {
		return
	}
	l.previousPage = true
	l.loadHistory(d, false)
}

func (l *liveState) browseRange(d *viewState, first, last, total int) {
	if l.loading || l.busy || l.key != d.liveQueryKey() || time.Now().Before(l.pageRetry) {
		return
	}
	if first >= 0 && first < 10 && l.windowOffset > 0 {
		l.loadPrevious(d)
		return
	}
	if last >= 0 && last+10 >= total && l.cursor != "" {
		l.loadHistory(d, true)
	}
}

// readHistoryWindow consumes Engine cursors one page at a time. Older reads
// append one page. Newer reads start at the closest retained checkpoint.
// Reconciliation starts a fresh traversal and locates a surviving old anchor.
// It retains only one bounded window even while scanning a distant anchor.
func readHistoryWindow(ctx context.Context, client *api.Client, q engine.HistorySearchRequest, more, previous bool, offset, target int, anchors []string, points []historyCheckpoint, revision uint64) (api.HistorySearchPage, int, []historyCheckpoint, error) {
	var out api.HistorySearchPage
	start := 0
	if more {
		p, err := client.SearchHistory(ctx, q)
		if err == nil && p.HistoryRevision != revision {
			return out, offset, points, fmt.Errorf("history changed during pagination")
		}
		return p, offset, points, err
	}
	wanted := offset
	if previous {
		wanted = max(0, (offset-50)/50*50)
		for _, p := range points {
			if p.offset <= wanted {
				start = p.offset
				q.Cursor = p.cursor
			}
		}
	} else {
		points = nil
	}
	found := previous || len(anchors) == 0
	for {
		points = rememberHistoryCheckpoint(points, historyCheckpoint{start, q.Cursor})
		page, err := client.SearchHistory(ctx, q)
		if err != nil {
			return out, offset, points, err
		}
		if out.HistoryRevision != 0 && out.HistoryRevision != page.HistoryRevision {
			return out, offset, points, fmt.Errorf("history changed during reconciliation")
		}
		if previous && page.HistoryRevision != revision {
			return out, offset, points, fmt.Errorf("history changed during pagination")
		}
		out.HistoryRevision = page.HistoryRevision
		out.IndexIncomplete = out.IndexIncomplete || page.IndexIncomplete
		out.EvaluatedAtMs = page.EvaluatedAtMs
		if !found {
			found = slices.ContainsFunc(page.Hits, func(h api.HistorySearchHit) bool { return slices.Contains(anchors, h.Item.ID) })
			if found {
				wanted = start
			}
		}
		if found && start >= wanted {
			out.Hits = append(out.Hits, page.Hits...)
		}
		start += len(page.Hits)
		out.NextCursor = page.NextCursor
		if found && (len(out.Hits) >= target || page.NextCursor == "") {
			return out, wanted, points, nil
		}
		if page.NextCursor == "" {
			// Every anchor was removed. Return the final bounded page explicitly.
			out.Hits = page.Hits
			return out, max(0, start-len(page.Hits)), points, nil
		}
		q.Cursor = page.NextCursor
	}
}
