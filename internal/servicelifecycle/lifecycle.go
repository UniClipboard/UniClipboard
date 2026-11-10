// Package servicelifecycle defines profile-scoped local service control.
package servicelifecycle

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"sync"
	"time"
)

// ExitDirectoryInUse lets the launcher wait for a retiring service without parsing errors.
const ExitDirectoryInUse = 75

// MaxLeases bounds simultaneous CLI usages of one service generation.
const MaxLeases = 64

// Mode says who owns the lifetime of a service generation.
type Mode string

const (
	// ModeStandalone is started by the user. Clients borrow it and never stop it.
	ModeStandalone Mode = "standalone"
	// ModeDesktop is owned by the normal desktop through a parent pipe.
	ModeDesktop Mode = "desktop"
	// ModeCLITemporary lives while at least one CLI lease is held.
	ModeCLITemporary Mode = "cli-temporary"
)

// Reasons returned in the Reason field of lifecycle errors.
const (
	ReasonRetiring          = "retiring"
	ReasonGenerationChanged = "generation_changed"
	ReasonLeaseCapacity     = "resource_exhausted"
)

// Instance identifies a service generation, not merely a reusable PID.
type Instance struct {
	ID          string `json:"id"`
	PID         int    `json:"pid"`
	Directory   string `json:"directory"`
	Mode        Mode   `json:"mode"`
	StartedAtMs int64  `json:"started_at_ms"`
	// Leases counts live CLI usages. Retiring generations admit no new lease.
	Leases   int  `json:"leases"`
	Retiring bool `json:"retiring"`
}

// WatchOwner cancels the service when its exclusive parent pipe closes.
// Only --desktop-owned uses stdin for this purpose. Standalone services ignore stdin.
func WatchOwner(input io.Reader, cancel context.CancelFunc) {
	_, _ = io.Copy(io.Discard, input)
	cancel()
}

// Service is the authoritative lifecycle state of one generation.
type Service struct {
	base   Instance
	cancel context.CancelFunc

	mu       sync.Mutex
	leases   map[string]chan struct{} // closed by an explicit release
	retiring bool
	initial  bool // the launcher still owns a temporary service
}

// New creates the lifecycle state. A temporary service starts owned by its launcher.
func New(base Instance, cancel context.CancelFunc) *Service {
	base.StartedAtMs = time.Now().UnixMilli()
	return &Service{base: base, cancel: cancel, leases: map[string]chan struct{}{}, initial: base.Mode == ModeCLITemporary}
}

// Info returns the current generation description.
func (s *Service) Info() Instance {
	s.mu.Lock()
	defer s.mu.Unlock()
	v := s.base
	v.Leases, v.Retiring = len(s.leases), s.retiring
	return v
}

// WatchInitialOwner releases the launcher's claim when its pipe closes.
// A claim that ends before any lease was admitted retires the service.
func (s *Service) WatchInitialOwner(input io.Reader) {
	_, _ = io.Copy(io.Discard, input)
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.initial && len(s.leases) == 0 {
		s.retireLocked()
	}
	s.initial = false
}

func (s *Service) retireLocked() {
	if !s.retiring {
		s.retiring = true
		s.cancel()
	}
}

type admission struct {
	id   string
	done chan struct{}
}

var (
	errRetiring = errors.New("service generation is retiring")
	errCapacity = errors.New("too many active usages")
)

func (s *Service) admit() (admission, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.retiring {
		return admission{}, errRetiring
	}
	if len(s.leases) >= MaxLeases {
		return admission{}, errCapacity
	}
	var raw [8]byte
	if _, err := rand.Read(raw[:]); err != nil {
		return admission{}, err
	}
	a := admission{id: hex.EncodeToString(raw[:]), done: make(chan struct{})}
	s.leases[a.id] = a.done
	s.initial = false
	return a, nil
}

// release removes a lease once. It reports whether this call retired the service.
func (s *Service) release(id string) (last, found bool, done chan struct{}) {
	s.mu.Lock()
	defer s.mu.Unlock()
	done, found = s.leases[id]
	if !found {
		return false, false, nil
	}
	delete(s.leases, id)
	if s.base.Mode == ModeCLITemporary && len(s.leases) == 0 && !s.initial {
		// Close admission now; the caller cancels after it has answered.
		s.retiring = true
		last = true
	}
	return last, true, done
}

func lifecycleError(w http.ResponseWriter, status int, message, reason string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(map[string]string{"error": message, "reason": reason})
}

func generationBody(w http.ResponseWriter, r *http.Request) (string, bool) {
	var request struct {
		ID string `json:"id"`
	}
	decoder := json.NewDecoder(io.LimitReader(r.Body, 4096))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&request); err != nil {
		lifecycleError(w, http.StatusBadRequest, "invalid request", "invalid_argument")
		return "", false
	}
	_, _ = io.Copy(io.Discard, io.LimitReader(r.Body, 4096))
	return request.ID, true
}

// Register adds same-user control to the private local socket.
func (s *Service) Register(mux *http.ServeMux) {
	mux.HandleFunc("GET /v1/service", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(s.Info())
	})
	mux.HandleFunc("POST /v1/service/stop", func(w http.ResponseWriter, r *http.Request) {
		var request struct {
			ID string `json:"id"`
		}
		decoder := json.NewDecoder(io.LimitReader(r.Body, 4096))
		decoder.DisallowUnknownFields()
		if err := decoder.Decode(&request); err != nil || request.ID != s.base.ID {
			http.Error(w, "service generation changed", http.StatusConflict)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_, _ = w.Write([]byte(`{"ok":true}`))
		if f, ok := w.(http.Flusher); ok {
			f.Flush()
		}
		s.cancel()
	})
	// A lease is the open response of this request. Its first line acknowledges admission.
	// Closing the connection, including by process death, releases the lease.
	mux.HandleFunc("POST /v1/service/leases", func(w http.ResponseWriter, r *http.Request) {
		id, ok := generationBody(w, r)
		if !ok {
			return
		}
		if id != s.base.ID {
			lifecycleError(w, http.StatusConflict, "service generation changed", ReasonGenerationChanged)
			return
		}
		a, err := s.admit()
		switch {
		case errors.Is(err, errRetiring):
			lifecycleError(w, http.StatusConflict, "service is retiring", ReasonRetiring)
			return
		case errors.Is(err, errCapacity):
			lifecycleError(w, http.StatusTooManyRequests, "too many active usages", ReasonLeaseCapacity)
			return
		case err != nil:
			lifecycleError(w, http.StatusInternalServerError, "cannot admit usage", "internal")
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]string{"lease": a.id})
		if f, ok := w.(http.Flusher); ok {
			f.Flush()
		}
		select {
		case <-r.Context().Done():
			if last, _, _ := s.release(a.id); last {
				s.cancel()
			}
		case <-a.done:
		}
	})
	// Explicit release tells the caller whether it retired a temporary generation.
	mux.HandleFunc("DELETE /v1/service/leases/{lease}", func(w http.ResponseWriter, r *http.Request) {
		id, ok := generationBody(w, r)
		if !ok {
			return
		}
		if id != s.base.ID {
			lifecycleError(w, http.StatusConflict, "service generation changed", ReasonGenerationChanged)
			return
		}
		lease := r.PathValue("lease")
		last, found, done := s.release(lease)
		if !found {
			lifecycleError(w, http.StatusNotFound, "no such usage", "not_found")
			return
		}
		close(done)
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(map[string]bool{"last": last})
		if f, ok := w.(http.Flusher); ok {
			f.Flush()
		}
		if last {
			s.cancel()
		}
	})
}
