package engine

import (
	"context"
	"io"
	"sync"
	"sync/atomic"
	"time"
)

// commitLocked records an observable change: it advances the revision and
// emits one event per kind. e.mu must be held. Delivery never blocks.
func (e *Engine) commitLocked(kinds ...EventKind) {
	e.commitObjsLocked(nil, kinds...)
}

func (e *Engine) commitObjsLocked(objs []ObjectRef, kinds ...EventKind) {
	for _, k := range kinds {
		e.rev++
		ev := Event{InstanceID: e.instance, Revision: e.rev, AtMs: time.Now().UnixMilli(), Kind: k,
			Objects: append([]ObjectRef(nil), objs...)}
		for s := range e.subs {
			s.deliver(ev)
		}
	}
}

func (e *Engine) commit(kinds ...EventKind) {
	e.mu.Lock()
	e.commitLocked(kinds...)
	e.mu.Unlock()
}

// Subscription delivers events after an initial snapshot. Close it when done.
type Subscription struct {
	e        *Engine
	snapshot State

	mu     sync.Mutex
	ch     chan Event
	closed bool
	resync bool
	notify chan struct{}
	busy   atomic.Bool
}

// SubscribeEvents registers for events and returns, atomically with that, the
// state they continue from: Next only returns changes after Snapshot, so
// nothing falls between a query and a subscription. ctx only bounds this call.
func (e *Engine) SubscribeEvents(ctx context.Context) (*Subscription, error) {
	release, gerr := e.enter(ctx)
	if gerr != nil {
		return nil, gerr
	}
	defer release()
	e.mu.Lock()
	defer e.mu.Unlock()
	if len(e.subs) >= e.res.MaxSubscriptions {
		return nil, newErr(CodeResourceExhausted, "too many subscriptions")
	}
	st, err := e.stateLocked()
	if err != nil {
		return nil, err
	}
	s := &Subscription{e: e, snapshot: st, ch: make(chan Event, e.res.EventBuffer), notify: make(chan struct{}, 1)}
	e.subs[s] = struct{}{}
	return s, nil
}

// Snapshot is the state this subscription continues from.
func (s *Subscription) Snapshot() State { return s.snapshot.clone() }

func (s *Subscription) wake() {
	select {
	case s.notify <- struct{}{}:
	default:
	}
}

// deliver queues ev, or, if the consumer fell too far behind, drops its backlog
// and marks it for resynchronization. It never blocks.
func (s *Subscription) deliver(ev Event) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed || s.resync {
		return
	}
	select {
	case s.ch <- ev:
	default:
		for len(s.ch) > 0 {
			<-s.ch
		}
		s.resync = true
	}
	s.wake()
}

// Next returns the next event. When the consumer fell behind it returns one
// resync_required error and the subscription ends: subscribe again and use the
// new Snapshot. After Close it returns io.EOF. One Next at a time.
func (s *Subscription) Next(ctx context.Context) (Event, error) {
	if ctx == nil {
		return Event{}, newErr(CodeInvalidArgument, "nil context")
	}
	if !s.busy.CompareAndSwap(false, true) {
		return Event{}, newErr(CodeInUse, "another Next is waiting on this subscription")
	}
	defer s.busy.Store(false)
	for {
		s.mu.Lock()
		switch {
		case s.closed:
			s.mu.Unlock()
			return Event{}, io.EOF
		case s.resync:
			s.resync, s.closed = false, true
			s.mu.Unlock()
			s.e.unsubscribe(s)
			return Event{}, newErr(CodeResyncRequired, "events were lost; subscribe again")
		}
		select {
		case ev := <-s.ch:
			s.mu.Unlock()
			return ev, nil
		default:
		}
		s.mu.Unlock()
		select {
		case <-s.notify:
		case <-ctx.Done():
			return Event{}, ctxErr(ctx.Err())
		}
	}
}

// Close ends the subscription and wakes a waiting Next. It is idempotent.
func (s *Subscription) Close() error {
	s.finish()
	s.e.unsubscribe(s)
	return nil
}

func (s *Subscription) finish() {
	s.mu.Lock()
	s.closed = true
	s.mu.Unlock()
	s.wake()
}

func (e *Engine) unsubscribe(s *Subscription) {
	e.mu.Lock()
	delete(e.subs, s)
	e.mu.Unlock()
}
