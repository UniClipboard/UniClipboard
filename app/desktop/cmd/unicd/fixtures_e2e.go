//go:build unic_e2e

package main

import (
	"context"
	"encoding/json"
	"log/slog"
	"net/http"
	"runtime"
	"strings"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/host"
	"github.com/uniclipboard/unic/engine"
)

type forbiddenLogValue struct{}

func (forbiddenLogValue) LogValue() slog.Value { panic("an unapproved LogValuer was evaluated") }

// Release builds have neither these routes nor the shortened export timers.
func testDiagnosticRoutes(mux *http.ServeMux, n *host.Node) {
	logger := n.Diagnostics().Logger()
	derived := logger.With("stage", "commit", "secret", "fixture-private-attribute")
	mux.HandleFunc("POST /test/logs", func(w http.ResponseWriter, _ *http.Request) {
		derived.Debug("pipeline.completed", "result", "succeeded", "code", forbiddenLogValue{})
		derived.Info("diagnostics.changed", "target", "local", "source", "config")
		derived.Warn("pipeline.failed", "code", "unavailable")
		derived.Error("host.failed", "code", "internal")
		derived.WithGroup("unapproved").Debug("pipeline.completed", "secret", "fixture-group-secret")
		logger.Info("fixture-private-message")
		w.WriteHeader(200)
	})
	mux.HandleFunc("POST /test/pressure", func(w http.ResponseWriter, _ *http.Request) {
		for i := 0; i < 100000; i++ {
			derived.Info("pipeline.completed", "result", "succeeded")
		}
		w.WriteHeader(200)
	})
	mux.HandleFunc("POST /test/invalid-record", func(w http.ResponseWriter, _ *http.Request) {
		n.Diagnostics().Record(context.Background(), engine.DiagnosticRecord{Stage: "private-invalid-stage", Result: "private-invalid-result", Backend: "private-plugin", ContentKind: "text"})
		w.WriteHeader(200)
	})
	mux.HandleFunc("GET /test/resources", func(w http.ResponseWriter, _ *http.Request) {
		var mem runtime.MemStats
		runtime.ReadMemStats(&mem)
		_ = json.NewEncoder(w).Encode(map[string]uint64{"goroutines": uint64(runtime.NumGoroutine()), "heap_alloc": mem.HeapAlloc, "heap_sys": mem.HeapSys})
	})
	mux.HandleFunc("POST /test/commit", func(w http.ResponseWriter, _ *http.Request) {
		ctx := context.Background()
		e := n.Engine()
		text := "fixture-private-clipboard"
		h, err := e.ImportContent(ctx, engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Size: int64(len(text)), Reader: strings.NewReader(text)}}})
		if err != nil {
			http.Error(w, "fixture import failed", 500)
			return
		}
		defer func() { _ = e.ReleaseContent(ctx, h.LeaseID) }()
		key := engine.RequestID("fixture-" + time.Now().Format("150405.000000000"))
		request := engine.CommitRequest{RequestID: key, LeaseID: h.LeaseID}
		a, err := e.CommitContent(ctx, request)
		if err != nil {
			http.Error(w, "fixture commit failed", 500)
			return
		}
		b, err := e.CommitContent(ctx, request)
		if err != nil || a.HistoryID != b.HistoryID {
			http.Error(w, "fixture idempotency failed", 500)
			return
		}
		_ = json.NewEncoder(w).Encode(a)
	})
}
