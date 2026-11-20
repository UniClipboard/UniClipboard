package desktopui

import (
	"context"
	"time"
)

const detailReadLimit = 4

type detailRead struct {
	cancel   context.CancelFunc
	obsolete bool
	edge     int
}

// prefetchDetails consumes inclusive item indices from the list or grid viewport.
// Store IDs so background reconciliation cannot silently shift the window.
func (l *liveState) prefetchDetails(d *viewState, items []entry, first, last int) {
	l.detailWindow = nil
	if first >= 0 && last >= first && first < len(items) {
		last = min(last, len(items)-1)
		for i := first; i <= last; i++ {
			l.detailWindow = append(l.detailWindow, items[i].id)
		}
		for i := max(0, first-10); i < first; i++ {
			l.detailWindow = append(l.detailWindow, items[i].id)
		}
		for i := last + 1; i <= min(len(items)-1, last+10); i++ {
			l.detailWindow = append(l.detailWindow, items[i].id)
		}
	}
	l.loadDetail(d)
}

// The active detail retains one payload even when its list row scrolls away.
func (l *liveState) detailWanted(d *viewState, id string) bool {
	if _, ok := l.items[id]; !ok {
		return false
	}
	if id == d.selected {
		return true
	}
	for _, wanted := range l.detailWindow {
		if wanted == id {
			return true
		}
	}
	return false
}

func copyDetail(dst *entry, src entry) {
	dst.text, dst.image, dst.rich = src.text, src.image, src.rich
	dst.picture = src.picture
	dst.contentError, dst.folder = src.contentError, src.folder
}

func (l *liveState) dropDetail(d *viewState, id string) {
	if read := l.detailReads[id]; read != nil {
		read.obsolete = true
		read.cancel()
	}
	if _, ok := l.details[id]; !ok {
		return
	}
	delete(l.details, id)
	if l.retained != nil && l.retained.id == id {
		copyDetail(l.retained, entry{})
	}
	for i := range d.entries {
		if d.entries[i].id == id {
			copyDetail(&d.entries[i], entry{})
		}
	}
	l.contentSerial++
}

func (l *liveState) resetDetails(d *viewState) {
	l.detailEpoch++
	l.detailWindow = nil
	for _, read := range l.detailReads {
		read.obsolete = true
		read.cancel()
	}
	for id := range l.details {
		l.dropDetail(d, id)
	}
	l.detailKey, l.detailBusy = "", false
}

// All cache and scheduler changes run on the view thread. Canceled reads keep
// their slot until completion, bounding work even during rapid scrolling.
func (l *liveState) loadDetail(d *viewState) {
	if l.key != d.liveQueryKey() || l.ctx.Err() != nil {
		return
	}
	edge := thumbnailEdge(len(l.detailWindow) + 1)
	wantedEdge := func(id string) int {
		if id == d.selected {
			return 0
		}
		return edge
	}
	for id, read := range l.detailReads {
		it := l.items[id]
		wrongTier := (it.Kind == "image" || it.Kind == "files") && ((read.edge == 0) != (wantedEdge(id) == 0) || (read.edge > wantedEdge(id) && wantedEdge(id) > 0))
		if !l.detailWanted(d, id) || wrongTier {
			read.obsolete = true
			read.cancel()
		}
	}
	for id, v := range l.details {
		if !l.detailWanted(d, id) || (id != d.selected && v.picture != nil && !v.picture.matches(edge)) {
			l.dropDetail(d, id)
		}
	}
	l.detailKey = d.selected
	_, cached := l.details[d.selected]
	_, exists := l.items[d.selected]
	l.detailBusy = exists && !cached
	if l.detailReads == nil {
		l.detailReads = map[string]*detailRead{}
	}
	// Reserve the next per-view job for selection instead of leaving it behind
	// four neighbors waiting for the process-wide request slot. Cancelled jobs
	// keep their slot until completion, so the four-job bound remains intact.
	if l.detailBusy && l.detailReads[d.selected] == nil && len(l.detailReads) >= detailReadLimit {
		cancelling := false
		for _, read := range l.detailReads {
			cancelling = cancelling || read.obsolete
		}
		if !cancelling {
			for _, read := range l.detailReads {
				if read.edge > 0 {
					read.obsolete = true
					read.cancel()
					break
				}
			}
		}
	}
	order := append([]string{d.selected}, l.detailWindow...)
	for _, id := range order {
		if len(l.detailReads) >= detailReadLimit {
			break
		}
		it, ok := l.items[id]
		if !ok {
			continue
		}
		if v, ok := l.details[id]; ok && (v.picture == nil || v.picture.matches(wantedEdge(id))) {
			continue
		}
		if _, ok := l.detailReads[id]; ok {
			continue
		}
		ctx, cancel := context.WithTimeout(l.ctx, 8*time.Second)
		read := &detailRead{cancel: cancel, edge: wantedEdge(id)}
		l.detailReads[id] = read
		epoch, key := l.detailEpoch, l.key
		go func() {
			defer cancel()
			v := loadSearchEntry(ctx, l.client, it, read.edge)
			apply := func() {
				delete(l.detailReads, id)
				current, present := l.items[id]
				if !read.obsolete && epoch == l.detailEpoch && key == d.liveQueryKey() && present && current.Revision == it.Revision && l.detailWanted(d, id) && (v.picture == nil || v.picture.matches(wantedEdge(id))) {
					l.details[id] = v
					if l.retained != nil && l.retained.id == id {
						copyDetail(l.retained, v)
					}
					for i := range d.entries {
						if d.entries[i].id == id {
							copyDetail(&d.entries[i], v)
						}
					}
					l.contentSerial++
				}
				l.loadDetail(d)
			}
			select {
			case l.results <- apply:
				l.wake()
			case <-l.ctx.Done():
			}
		}()
	}
}
