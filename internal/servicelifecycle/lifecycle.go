// Package servicelifecycle defines profile-scoped local service control.
package servicelifecycle

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
)

// ExitDirectoryInUse lets the launcher wait for a retiring service without parsing errors.
const ExitDirectoryInUse = 75

// Instance identifies a service generation, not merely a reusable PID.
type Instance struct {
	ID           string `json:"id"`
	PID          int    `json:"pid"`
	Directory    string `json:"directory"`
	DesktopOwned bool   `json:"desktop_owned"`
}

// WatchOwner cancels the service when its exclusive parent pipe closes.
// Only --desktop-owned uses stdin for this purpose. Standalone services ignore stdin.
func WatchOwner(input io.Reader, cancel context.CancelFunc) {
	_, _ = io.Copy(io.Discard, input)
	cancel()
}

// Register adds same-user control to the private local socket.
func Register(mux *http.ServeMux, instance Instance, cancel context.CancelFunc) {
	mux.HandleFunc("GET /v1/service", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(instance)
	})
	mux.HandleFunc("POST /v1/service/stop", func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			ID string `json:"id"`
		}
		decoder := json.NewDecoder(io.LimitReader(r.Body, 4096))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&request); err != nil || request.ID != instance.ID {
			http.Error(w, "service generation changed", http.StatusConflict)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"ok":true}`))
		if f, ok := w.(http.Flusher); ok {
			f.Flush()
		}
		cancel()
	})
}
