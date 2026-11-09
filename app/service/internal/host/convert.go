package host

import (
	"sync"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/localcontent"
)

type pathSource = localcontent.PathSource

var contentInput = localcontent.Input

// ---- event hub ----

type hub struct {
	mu   sync.Mutex
	subs map[chan api.Event]struct{}
}

func (h *hub) subscribe() (<-chan api.Event, func()) {
	ch := make(chan api.Event, 32)
	h.mu.Lock()
	if h.subs == nil {
		h.subs = map[chan api.Event]struct{}{}
	}
	h.subs[ch] = struct{}{}
	h.mu.Unlock()
	return ch, func() { h.mu.Lock(); delete(h.subs, ch); h.mu.Unlock() }
}

// emit never blocks: a slow subscriber misses events instead of stalling the daemon.
func (h *hub) emit(ev api.Event) {
	ev.Time = time.Now().Unix()
	h.mu.Lock()
	defer h.mu.Unlock()
	for ch := range h.subs {
		select {
		case ch <- ev:
		default:
		}
	}
}
