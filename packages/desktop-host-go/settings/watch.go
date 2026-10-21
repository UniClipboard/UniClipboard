package settings

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// Daemon WS names (crates/uc-daemon-contract/src/constants.rs).
const (
	topicSettings          = "settings"
	topicSystem            = "system"
	eventSettingsSnapshot  = "settings.snapshot"
	eventSettingsChanged   = "settings.changed"
	eventSystemRefreshNeed = "system.refresh_required"
)

const (
	readTimeout     = 10 * time.Second
	snapshotTimeout = 10 * time.Second
	minBackoff      = 500 * time.Millisecond
	maxBackoff      = 30 * time.Second
)

// Watcher keeps a host's settings current. On every connection it subscribes, waits for
// `settings.snapshot`, reads `GET /settings`, and reads again after each newer `settings.changed`
// or `system.refresh_required`. Apply runs on the watcher's goroutine only, one call at a time,
// and must be idempotent: it is called after every read, including reads caused by the host's own
// writes.
type Watcher struct {
	client func() *daemonclient.Client // asked again on every attempt: the daemon may have been replaced
	apply  func(View)

	mu     sync.Mutex
	paused bool
	resume chan struct{} // closed by Resume
	ws     *daemonclient.WS
	cancel context.CancelFunc
}

// NewWatcher builds a watcher. client returns the current daemon client, or nil while there is
// none.
func NewWatcher(client func() *daemonclient.Client, apply func(View)) *Watcher {
	return &Watcher{client: client, apply: apply, resume: make(chan struct{})}
}

// Run connects and reconnects until ctx ends.
func (w *Watcher) Run(ctx context.Context) {
	backoff := minBackoff
	for ctx.Err() == nil {
		if resume := w.waitChannel(); resume != nil {
			select {
			case <-resume:
			case <-ctx.Done():
				return
			}
			continue
		}
		synced, err := w.session(ctx)
		if ctx.Err() != nil {
			return
		}
		if w.waitChannel() != nil {
			continue // paused on purpose: no backoff
		}
		if synced {
			backoff = minBackoff
		}
		log.Printf("settings watcher: connection ended (%v); reconnecting in %s", err, backoff)
		select {
		case <-time.After(backoff):
		case <-ctx.Done():
			return
		}
		backoff = min(backoff*2, maxBackoff)
	}
}

// Pause closes the connection with a normal close frame and keeps it closed until Resume. Call it
// before stopping the daemon: an open connection holds a control lease and the daemon waits for
// its HTTP server to finish.
func (w *Watcher) Pause() {
	w.mu.Lock()
	w.paused = true
	ws, cancel := w.ws, w.cancel
	w.ws, w.cancel = nil, nil
	w.mu.Unlock()
	if ws != nil {
		ws.Close()
	}
	if cancel != nil {
		cancel()
	}
}

// Resume lets the watcher connect again.
func (w *Watcher) Resume() {
	w.mu.Lock()
	defer w.mu.Unlock()
	if !w.paused {
		return
	}
	w.paused = false
	close(w.resume)
	w.resume = make(chan struct{})
}

// waitChannel returns the channel to wait on while paused, or nil when not paused.
func (w *Watcher) waitChannel() chan struct{} {
	w.mu.Lock()
	defer w.mu.Unlock()
	if w.paused {
		return w.resume
	}
	return nil
}

// session runs one connection. synced reports whether it got as far as the first read.
func (w *Watcher) session(ctx context.Context) (synced bool, err error) {
	client := w.client()
	if client == nil {
		return false, errors.New("no daemon connection")
	}
	connCtx, cancel := context.WithCancel(ctx)
	defer cancel()
	dialCtx, dialCancel := context.WithTimeout(connCtx, readTimeout)
	ws, err := client.DialWS(dialCtx)
	dialCancel()
	if err != nil {
		return false, err
	}
	w.mu.Lock()
	if w.paused {
		w.mu.Unlock()
		ws.Close()
		return false, errors.New("paused")
	}
	w.ws, w.cancel = ws, cancel
	w.mu.Unlock()
	defer func() {
		w.mu.Lock()
		if w.ws == ws {
			w.ws, w.cancel = nil, nil
			w.mu.Unlock()
			ws.Close()
			return
		}
		w.mu.Unlock() // Pause already closed it
	}()

	if err := ws.Subscribe(connCtx, topicSettings, topicSystem); err != nil {
		return false, err
	}
	events := ws.Events(connCtx)
	baseline, err := waitSnapshot(events)
	if err != nil {
		return false, err
	}
	if err := w.refresh(connCtx, client); err != nil {
		return false, err
	}
	for event := range events {
		stale := baseline.newer(event)
		// Coalesce: everything already received is answered by the same read.
		for drained := true; drained; {
			select {
			case next, ok := <-events:
				if !ok {
					return true, errors.New("connection closed")
				}
				stale = baseline.newer(next) || stale
			default:
				drained = false
			}
		}
		if stale {
			if err := w.refresh(connCtx, client); err != nil {
				return true, err
			}
		}
	}
	return true, errors.New("connection closed")
}

func (w *Watcher) refresh(ctx context.Context, client *daemonclient.Client) error {
	readCtx, cancel := context.WithTimeout(ctx, readTimeout)
	defer cancel()
	view, err := Read(readCtx, client)
	if err != nil {
		return fmt.Errorf("read settings: %w", err)
	}
	w.apply(view)
	return nil
}

// revisions are each owner's last known revision on one connection. Revisions of another
// connection (another daemon run) are never compared.
type revisions map[string]uint64

func waitSnapshot(events <-chan daemonclient.Event) (revisions, error) {
	timeout := time.After(snapshotTimeout)
	for {
		select {
		case event, ok := <-events:
			if !ok {
				return nil, errors.New("connection closed before settings.snapshot")
			}
			if event.Type != eventSettingsSnapshot {
				continue // a refresh request before the snapshot is answered by the first read
			}
			var payload struct {
				Revisions revisions `json:"revisions"`
			}
			if err := json.Unmarshal(event.Payload, &payload); err != nil || payload.Revisions == nil {
				return nil, errors.New("unreadable settings.snapshot")
			}
			return payload.Revisions, nil
		case <-timeout:
			return nil, errors.New("no settings.snapshot after subscribing")
		}
	}
}

// newer reports whether event means the settings must be read again, and records its revision.
func (r revisions) newer(event daemonclient.Event) bool {
	switch event.Type {
	case eventSystemRefreshNeed:
		return true
	case eventSettingsChanged:
		var payload struct {
			Owner    string `json:"owner"`
			Revision uint64 `json:"revision"`
		}
		if json.Unmarshal(event.Payload, &payload) != nil {
			return true // unreadable: reading again is always safe
		}
		if known, ok := r[payload.Owner]; ok && payload.Revision <= known {
			return false // already covered by a read after the snapshot
		}
		r[payload.Owner] = payload.Revision
		return true
	}
	return false
}
