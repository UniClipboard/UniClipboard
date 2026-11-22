package desktopui

import (
	"context"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"testing"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
)

// Exercise the actual Desktop intent worker and local HTTP client without a GUI.
func TestActionWorkerBudgetE2E(t *testing.T) {
	dir, err := os.MkdirTemp("", "unic-action-budget-")
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := os.RemoveAll(dir); err != nil {
			t.Error(err)
		}
	}()
	socket := filepath.Join(dir, "s")
	listener, err := net.Listen("unix", socket)
	if err != nil {
		t.Fatal(err)
	}
	server := &http.Server{Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		select {
		case <-r.Context().Done():
			return
		case <-time.After(16 * time.Second):
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"applied":true}`))
	})}
	defer func() { _ = server.Close() }()
	go func() { _ = server.Serve(listener) }()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	live := &liveState{ctx: ctx, client: api.NewClient(socket), jobs: make(chan func(context.Context) func(), 2), results: make(chan func(), 2)}
	budgets := make(chan time.Duration, 1)
	live.client.WrapTransport(func(base http.RoundTripper) http.RoundTripper {
		return actionBudgetTransport{base: base, budgets: budgets}
	})
	d := &viewState{live: live}
	go live.work()
	d.runHistoryAction("synthetic-history", "example.content/share")
	select {
	case apply := <-live.results:
		apply()
	case <-time.After(25 * time.Second):
		t.Fatal("worker did not complete")
	}
	if budget := <-budgets; budget < 5*time.Minute+5*time.Second || budget > 5*time.Minute+30*time.Second {
		t.Fatalf("Action budget is insufficient or unbounded: %v", budget)
	}
	if live.message != "" || d.notice != "已复制" {
		t.Fatalf("long Action did not complete: %s", live.message)
	}
	observed := make(chan time.Duration, 1)
	d.mutate(func(ctx context.Context) error {
		deadline, ok := ctx.Deadline()
		if !ok {
			t.Error("mutation has no deadline")
		}
		observed <- time.Until(deadline)
		return nil
	}, "done")
	select {
	case apply := <-live.results:
		apply()
	case <-time.After(25 * time.Second):
		t.Fatal("worker did not complete")
	}
	if budget := <-observed; budget <= 0 || budget > 15*time.Second {
		t.Fatalf("ordinary mutation budget changed: %v", budget)
	}
}

type actionBudgetTransport struct {
	base    http.RoundTripper
	budgets chan time.Duration
}

func (t actionBudgetTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	deadline, ok := r.Context().Deadline()
	budget := time.Duration(0)
	if ok {
		budget = time.Until(deadline)
	}
	t.budgets <- budget
	return t.base.RoundTrip(r)
}
