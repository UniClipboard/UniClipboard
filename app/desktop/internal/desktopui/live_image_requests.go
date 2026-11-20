package desktopui

import (
	"context"
	"slices"
	"sync"
)

// One request owns service materialization and image decode scratch across views.
// Admission must precede HistoryContent: its response already retains content.
var imageRequests imageRequestQueue

type imageRequestWaiter struct {
	ready    chan struct{}
	selected bool
}

type imageRequestQueue struct {
	mu      sync.Mutex
	busy    bool
	waiting []*imageRequestWaiter
}

func (q *imageRequestQueue) acquire(ctx context.Context, selected bool) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	q.mu.Lock()
	if !q.busy {
		q.busy = true
		q.mu.Unlock()
		return nil
	}
	waiter := &imageRequestWaiter{ready: make(chan struct{}), selected: selected}
	q.waiting = append(q.waiting, waiter)
	q.mu.Unlock()
	select {
	case <-waiter.ready:
		if err := ctx.Err(); err != nil {
			q.release()
			return err
		}
		return nil
	case <-ctx.Done():
		q.mu.Lock()
		for i, w := range q.waiting {
			if w == waiter {
				q.waiting = slices.Delete(q.waiting, i, i+1)
				q.mu.Unlock()
				return ctx.Err()
			}
		}
		// Release won the race and transferred ownership to this cancelled waiter.
		q.mu.Unlock()
		q.release()
		return ctx.Err()
	}
}

func (q *imageRequestQueue) release() {
	q.mu.Lock()
	defer q.mu.Unlock()
	if len(q.waiting) == 0 {
		q.busy = false
		return
	}
	next := 0
	for i, w := range q.waiting {
		if w.selected {
			next = i
			break
		}
	}
	waiter := q.waiting[next]
	q.waiting = slices.Delete(q.waiting, next, next+1)
	close(waiter.ready)
}
