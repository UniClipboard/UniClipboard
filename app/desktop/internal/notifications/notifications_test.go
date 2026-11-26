package notifications

import (
	"context"
	"encoding/json"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	config "github.com/uniclipboard/unic/internal/localconfig"
)

type fakeDeliverer struct {
	mu        sync.Mutex
	supported bool
	messages  []Message
	err       error
}

func (f *fakeDeliverer) Supported() bool { return f.supported }

func (f *fakeDeliverer) Deliver(_ context.Context, msg Message) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.err != nil {
		return f.err
	}
	f.messages = append(f.messages, msg)
	return nil
}

func (f *fakeDeliverer) list() []Message {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]Message, len(f.messages))
	copy(out, f.messages)
	return out
}

func boolPtr(v bool) *bool { return &v }

func TestServicePolicyCooldownAndRouting(t *testing.T) {
	del := &fakeDeliverer{supported: true}
	cfg := config.Default().Desktop
	var navigated []string

	now := time.Unix(1000, 0)
	svc := New(Options{
		Deliverer: del,
		Config:    func() config.Desktop { return cfg },
		OnNavigate: func(route string) {
			navigated = append(navigated, route)
		},
		Cooldown: 5 * time.Second,
	})
	svc.now = func() time.Time { return now }

	msg := BuildPendingMessage("zh-CN", "dev-1", "M02 Physical", "code")
	ok, err := svc.Notify(context.Background(), msg)
	if err != nil || !ok {
		t.Fatalf("first notify = (%v, %v), want (true, nil)", ok, err)
	}
	// Within cooldown, same ID is deduplicated.
	ok, err = svc.Notify(context.Background(), msg)
	if err != nil || ok {
		t.Fatalf("duplicate notify within cooldown = (%v, %v), want (false, nil)", ok, err)
	}
	// After cooldown, same ID can deliver again.
	now = now.Add(6 * time.Second)
	ok, err = svc.Notify(context.Background(), msg)
	if err != nil || !ok {
		t.Fatalf("notify after cooldown = (%v, %v), want (true, nil)", ok, err)
	}
	if len(del.list()) != 2 {
		t.Fatalf("delivered count = %d, want 2", len(del.list()))
	}

	// Disabling device_pairing blocks CategoryDevicePairing.
	cfg.Notifications.DevicePairing = boolPtr(false)
	now = now.Add(6 * time.Second)
	ok, err = svc.Notify(context.Background(), msg)
	if err != nil || ok {
		t.Fatalf("disabled category notify = (%v, %v), want (false, nil)", ok, err)
	}

	// Master switch off blocks even when device_pairing is true.
	cfg.Notifications.Enabled = boolPtr(false)
	cfg.Notifications.DevicePairing = boolPtr(true)
	ok, err = svc.Notify(context.Background(), msg)
	if err != nil || ok {
		t.Fatalf("disabled master notify = (%v, %v), want (false, nil)", ok, err)
	}

	// Click routing works for recorded IDs and deterministic fallbacks.
	svc.HandleClick(msg.ID)
	svc.HandleClick("pending:id:dev-99")
	svc.HandleClick("pending:peer:Phone")
	svc.HandleClick("test:abc")
	wantRoutes := []string{"devices:dev-1", "devices:dev-99", "devices", "settings:notifications"}
	if len(navigated) != len(wantRoutes) {
		t.Fatalf("navigated = %v, want %v", navigated, wantRoutes)
	}
	for i := range wantRoutes {
		if navigated[i] != wantRoutes[i] {
			t.Fatalf("navigated[%d] = %q, want %q", i, navigated[i], wantRoutes[i])
		}
	}
}

func TestServiceWatchPendingAndClear(t *testing.T) {
	dir, err := os.MkdirTemp("/tmp", "unic-notif-")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.RemoveAll(dir) })
	sock := filepath.Join(dir, "u.sock")
	ln, err := net.Listen("unix", sock)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = ln.Close() }()

	var mu sync.Mutex
	pending := []api.PendingPeer{}
	eventsCh := make(chan api.Event, 8)

	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/status", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		st := api.Status{Pending: append([]api.PendingPeer(nil), pending...)}
		mu.Unlock()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(st)
	})
	mux.HandleFunc("GET /v1/events", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/event-stream")
		flusher, _ := w.(http.Flusher)
		for {
			select {
			case <-r.Context().Done():
				return
			case ev := <-eventsCh:
				raw, _ := json.Marshal(ev)
				_, _ = w.Write([]byte("data: "))
				_, _ = w.Write(raw)
				_, _ = w.Write([]byte("\n\n"))
				if flusher != nil {
					flusher.Flush()
				}
			}
		}
	})
	srv := &http.Server{Handler: mux}
	go func() { _ = srv.Serve(ln) }()
	defer func() { _ = srv.Close() }()

	del := &fakeDeliverer{supported: true}
	svc := New(Options{
		Deliverer: del,
		Config:    func() config.Desktop { return config.Default().Desktop },
	})

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	summaries := make(chan PendingSummary, 8)
	go svc.Watch(ctx, api.NewClient(sock), func(s PendingSummary) {
		summaries <- s
	})

	// Initial empty summary.
	select {
	case s := <-summaries:
		if s.Count != 0 {
			t.Fatalf("initial summary = %+v, want 0", s)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("timed out waiting for initial summary")
	}

	// Simulate incoming pending device request.
	mu.Lock()
	pending = []api.PendingPeer{{ID: "dev-m02", Name: "M02 Physical", ByCode: true}}
	mu.Unlock()
	eventsCh <- api.Event{Kind: "pending", Peer: "M02 Physical", Info: "code"}

	select {
	case s := <-summaries:
		if s.Count != 1 || s.FirstID != "dev-m02" {
			t.Fatalf("pending summary = %+v, want count=1 dev-m02", s)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("timed out waiting for pending summary")
	}

	deadline := time.Now().Add(3 * time.Second)
	for len(del.list()) == 0 && time.Now().Before(deadline) {
		time.Sleep(10 * time.Millisecond)
	}
	msgs := del.list()
	if len(msgs) != 1 || msgs[0].Route != "devices:dev-m02" {
		t.Fatalf("delivered messages = %+v, want 1 routed to devices:dev-m02", msgs)
	}

	// Simulate approval clearing pending list.
	mu.Lock()
	pending = nil
	mu.Unlock()
	eventsCh <- api.Event{Kind: "peers"}

	select {
	case s := <-summaries:
		if s.Count != 0 {
			t.Fatalf("cleared summary = %+v, want 0", s)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("timed out waiting for cleared summary")
	}
}
