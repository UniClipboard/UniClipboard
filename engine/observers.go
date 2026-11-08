package engine

import (
	"context"
	"slices"
	"sync"
	"time"

	"github.com/uniclipboard/unic/plugin"
)

const (
	observerQueue   = 64
	observerTimeout = 200 * time.Millisecond
	observerDrain   = time.Second
)

var observerEvents = []string{plugin.EventCommitted, plugin.EventUpdated, plugin.EventDeleted, plugin.EventExpired,
	plugin.EventPasted, plugin.EventAppStarted, plugin.EventAppShutdown}

// pipelineObserver delivers history notifications to one plugin, one at a
// time, from its own goroutine. Events are hints: when the plugin is slow the
// queue drops the newest rather than ever holding up the engine, and a failed
// or timed-out delivery is forgotten.
type pipelineObserver struct {
	id     string
	events []string
	fn     func(context.Context, plugin.Event) error
	queue  chan plugin.Event
	stop   chan struct{}
	done   chan struct{}
	once   sync.Once
}

func newPipelineObserver(id string, events []string, fn func(context.Context, plugin.Event) error) *pipelineObserver {
	o := &pipelineObserver{id: id, events: events, fn: fn,
		queue: make(chan plugin.Event, observerQueue), stop: make(chan struct{}), done: make(chan struct{})}
	go o.run()
	return o
}

func (o *pipelineObserver) run() {
	defer close(o.done)
	for {
		select {
		case <-o.stop:
			o.drain()
			return
		case ev := <-o.queue:
			ctx, cancel := context.WithTimeout(context.Background(), observerTimeout)
			o.call(ctx, ev)
			cancel()
		}
	}
}

// drain delivers what is still queued, e.g. app.shutdown, within a short
// overall allowance.
func (o *pipelineObserver) drain() {
	deadline := time.Now().Add(observerDrain)
	for time.Now().Before(deadline) {
		select {
		case ev := <-o.queue:
			ctx, cancel := context.WithDeadline(context.Background(), minTime(deadline, time.Now().Add(observerTimeout)))
			o.call(ctx, ev)
			cancel()
		default:
			return
		}
	}
}

func minTime(a, b time.Time) time.Time {
	if a.Before(b) {
		return a
	}
	return b
}

func (o *pipelineObserver) call(ctx context.Context, ev plugin.Event) {
	defer func() { _ = recover() }()
	_ = o.fn(ctx, ev) // only the fact of a failure could be logged, never its text
}

func (o *pipelineObserver) close() {
	o.once.Do(func() { close(o.stop) })
	select {
	case <-o.done:
	case <-time.After(observerDrain + observerTimeout + time.Second): // a callback that ignores its context cannot hold the engine
	}
}

// notifyObservers queues one event per ID for every subscribed observer.
// It never blocks and takes no engine lock.
func (e *Engine) notifyObservers(kind string, ids ...string) {
	if len(e.observers) == 0 {
		return
	}
	at := time.Now().UnixMilli()
	for _, o := range e.observers {
		if !slices.Contains(o.events, kind) {
			continue
		}
		for _, id := range ids {
			select {
			case o.queue <- plugin.Event{Kind: kind, ItemID: id, AtMs: at}:
			default:
			}
		}
	}
}

func (e *Engine) stopObservers() {
	for _, o := range e.observers {
		o.close()
	}
	e.observers = nil
}
