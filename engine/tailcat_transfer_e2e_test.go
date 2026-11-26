package engine

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/plugins/tailcat"
	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// Pause after Engine accepts connected, before the callback returns to the
// backend. A scheduler can pause this goroutine at exactly the same boundary.
type connectedBarrier struct {
	plugin.PeerAuthority
	once    sync.Once
	unblock sync.Once
	entered chan struct{}
	release chan struct{}
}

func (a *connectedBarrier) resume() { a.unblock.Do(func() { close(a.release) }) }

func (a *connectedBarrier) PeerEvent(ctx context.Context, ev rpc.PeerEvent) error {
	err := a.PeerAuthority.PeerEvent(ctx, ev)
	if err == nil && ev.State == "connected" {
		a.once.Do(func() {
			close(a.entered)
			select {
			case <-a.release:
			case <-ctx.Done():
			}
		})
	}
	return err
}

type observedTailcat struct {
	*tailcat.Plugin
	barrier *connectedBarrier
	mu      sync.Mutex
	sendErr string
	started chan struct{}
	ended   chan struct{}
}

func (p *observedTailcat) SetPeerAuthority(a plugin.PeerAuthority) {
	p.barrier.PeerAuthority = a
	p.Plugin.SetPeerAuthority(p.barrier)
}

func (p *observedTailcat) Send(ctx context.Context, in rpc.Send, src plugin.SendSource) (rpc.SendResult, error) {
	close(p.started)
	defer close(p.ended)
	res, err := p.Plugin.Send(ctx, in, src)
	code := "success"
	if err != nil {
		code = "unclassified"
		var re *rpc.Error
		if errors.As(err, &re) {
			code = re.Code
		}
	}
	p.mu.Lock()
	p.sendErr = code
	p.mu.Unlock()
	return res, err
}

func TestTailcatConnectedPublicationDeliversEverything(t *testing.T) {
	a := &connectedBarrier{entered: make(chan struct{}), release: make(chan struct{})}
	defer a.resume()
	var observed *observedTailcat
	n, receiver := senderNode(t, func(o *Options) {
		observed = &observedTailcat{Plugin: o.Plugins[0].Plugin.(*tailcat.Plugin), barrier: a,
			started: make(chan struct{}), ended: make(chan struct{})}
		o.Plugins[0].Plugin = observed
	})
	select {
	case <-a.entered:
	case <-time.After(5 * time.Second):
		t.Fatal("connected publication barrier was not reached")
	}
	ctx, cancel := context.WithCancel(context.Background())
	controllerDone := make(chan struct{})
	go func() {
		defer close(controllerDone)
		select {
		case <-observed.started:
			// Let a missing-session error surface before resuming publication.
			// A send that waits for publication can proceed after the barrier.
			select {
			case <-observed.ended:
			case <-time.After(100 * time.Millisecond):
			case <-ctx.Done():
			}
		case <-ctx.Done():
		}
		a.resume()
	}()
	defer func() { cancel(); a.resume(); <-controllerDone }()
	defer func() {
		observed.mu.Lock()
		code := observed.sendErr
		observed.mu.Unlock()
		t.Logf("connected-publication backend send code: %s", code)
		if dest := os.Getenv("UNIC_E2E_ARTIFACTS"); dest != "" {
			result := "PASS"
			if t.Failed() {
				result = "FAIL"
			}
			data, err := json.MarshalIndent(map[string]any{
				"test": t.Name(), "result": result, "backend_send_code": code,
				"barrier":   "after_engine_connected_before_callback_return",
				"transport": "loopback_tcp", "payload": "text_html_png_inline_file_referenced_file",
			}, "", "  ")
			if err == nil {
				err = os.MkdirAll(dest, 0o700)
			}
			if err == nil {
				err = os.WriteFile(filepath.Join(dest, "connected-publication.json"), data, 0o600)
			}
			if err != nil {
				t.Errorf("write E2E receipt: %v", err)
			}
		}
	}()
	assertSubmitDeliversEverything(t, n, receiver)
}
