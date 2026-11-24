package mobile

import (
	"context"
	"sync"
)

// CancelToken bridges cancellation across the gomobile boundary.
type CancelToken struct {
	mu        sync.Mutex
	cancelled bool
	cancels   map[int]context.CancelFunc
	nextID    int
}

// NewCancelToken creates a new cancellable token.
func NewCancelToken() *CancelToken {
	return &CancelToken{cancels: make(map[int]context.CancelFunc)}
}

// Cancel marks the token as cancelled and aborts all associated active calls.
func (t *CancelToken) Cancel() {
	if t == nil {
		return
	}
	t.mu.Lock()
	if t.cancelled {
		t.mu.Unlock()
		return
	}
	t.cancelled = true
	fns := make([]context.CancelFunc, 0, len(t.cancels))
	for _, fn := range t.cancels {
		fns = append(fns, fn)
	}
	t.cancels = nil
	t.mu.Unlock()
	for _, fn := range fns {
		fn()
	}
}

func withToken(t *CancelToken) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(context.Background())
	if t == nil {
		return ctx, cancel
	}
	t.mu.Lock()
	if t.cancelled {
		t.mu.Unlock()
		cancel()
		return ctx, cancel
	}
	t.nextID++
	id := t.nextID
	if t.cancels == nil {
		t.cancels = make(map[int]context.CancelFunc)
	}
	t.cancels[id] = cancel
	t.mu.Unlock()
	return ctx, func() {
		t.mu.Lock()
		if t.cancels != nil {
			delete(t.cancels, id)
		}
		t.mu.Unlock()
		cancel()
	}
}
