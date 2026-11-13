package desktopui

import (
	"context"
	"time"
)

const detailReadLimit = 4

type detailRead struct {
	cancel   context.CancelFunc
	obsolete bool
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
	for id, read := range l.detailReads {
		if !l.detailWanted(d, id) {
			read.obsolete = true
			read.cancel()
		}
	}
	for id := range l.details {
		if !l.detailWanted(d, id) {
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
	order := append([]string{d.selected}, l.detailWindow...)
	for _, id := range order {
		if len(l.detailReads) >= detailReadLimit {
			break
		}
		it, ok := l.items[id]
		if !ok {
			continue
		}
		if _, ok := l.details[id]; ok {
			continue
		}
		if _, ok := l.detailReads[id]; ok {
			continue
		}
		ctx, cancel := context.WithTimeout(l.ctx, 8*time.Second)
		read := &detailRead{cancel: cancel}
		l.detailReads[id] = read
		epoch, key := l.detailEpoch, l.key
		go func() {
			defer cancel()
			v := loadSearchEntry(ctx, l.client, it)
			apply := func() {
				delete(l.detailReads, id)
				current, present := l.items[id]
				if !read.obsolete && epoch == l.detailEpoch && key == d.liveQueryKey() && present && current.Revision == it.Revision && l.detailWanted(d, id) {
					l.details[id] = v
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
			case <-l.ctx.Done():
			}
		}()
	}
}
