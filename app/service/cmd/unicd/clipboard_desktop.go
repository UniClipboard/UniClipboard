//go:build darwin || linux || windows

package main

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/clipboardintegration"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/nativeboard"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

var openDesktopBoard = func() (clipboardintegration.Board, error) { return nativeboard.New() }

func registerDesktopClipboard(ctx context.Context, mux *http.ServeMux, socket string, lifecycle *servicelifecycle.Service) func() {
	var mu sync.Mutex
	var board *clipboardintegration.TrackedBoard
	var stopped chan struct{}
	var ready chan struct{}
	client := api.NewClient(socket)
	mux.HandleFunc("POST /v1/desktop/clipboard/enable", func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		state := lifecycle.Info()
		if ctx.Err() != nil || state.Retiring || state.Mode == servicelifecycle.ModeCLITemporary {
			mu.Unlock()
			http.Error(w, "desktop clipboard is unavailable", http.StatusConflict)
			return
		}
		if board == nil {
			native, err := openDesktopBoard()
			if err != nil {
				mu.Unlock()
				if errors.Is(err, nativeboard.ErrDesktopClipboardUnsupported) {
					w.Header().Set("Content-Type", "application/json")
					w.WriteHeader(http.StatusNotImplemented)
					_ = json.NewEncoder(w).Encode(api.ErrorBody{
						Error:  "background clipboard is unavailable in this desktop session",
						Reason: api.ReasonClipboardUnsupported,
					})
				} else {
					http.Error(w, "desktop clipboard is unavailable", http.StatusServiceUnavailable)
				}
				return
			}
			tracked := clipboardintegration.NewTrackedBoard(native)
			board = tracked
			stopped = make(chan struct{})
			ready = make(chan struct{})
			go func() {
				defer close(stopped)
				_ = clipboardintegration.RunRemoteReady(ctx, client, tracked, ready)
			}()
		}
		awaitReady := ready
		mu.Unlock()
		deadline, cancel := context.WithTimeout(r.Context(), 10*time.Second)
		defer cancel()
		select {
		case <-awaitReady:
			w.WriteHeader(http.StatusOK)
		case <-deadline.Done():
			http.Error(w, "desktop clipboard is unavailable", http.StatusServiceUnavailable)
		case <-ctx.Done():
			http.Error(w, "desktop clipboard is unavailable", http.StatusServiceUnavailable)
		}
	})
	getBoard := func() *clipboardintegration.TrackedBoard {
		mu.Lock()
		defer mu.Unlock()
		return board
	}
	mux.HandleFunc("POST /v1/desktop/clipboard/apply", func(w http.ResponseWriter, r *http.Request) {
		var req api.ClipboardApplicationRequest
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&req); err != nil || req.ID == "" {
			http.Error(w, "invalid clipboard request", http.StatusBadRequest)
			return
		}
		out := api.ClipboardApplication{Failure: "clipboard"}
		if tracked := getBoard(); tracked != nil {
			mark, err := tracked.Apply(r.Context(), client, req.ID, req.Plain)
			switch {
			case err == nil:
				out = api.ClipboardApplication{Mark: mark}
			case errors.Is(err, clipboardintegration.ErrPlain):
				out.Failure = "plain"
			case !errors.Is(err, clipboardintegration.ErrClipboard):
				out.Failure = "target"
			}
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(out)
	})
	mux.HandleFunc("POST /v1/desktop/clipboard/match", func(w http.ResponseWriter, r *http.Request) {
		var mark string
		if err := json.NewDecoder(http.MaxBytesReader(w, r.Body, 4096)).Decode(&mark); err != nil {
			http.Error(w, "invalid clipboard mark", http.StatusBadRequest)
			return
		}
		tracked := getBoard()
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(tracked != nil && tracked.Matches(mark))
	})
	return func() {
		mu.Lock()
		done := stopped
		mu.Unlock()
		if done != nil {
			<-done
		}
	}
}
