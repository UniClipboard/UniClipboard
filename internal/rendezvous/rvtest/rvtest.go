// Package rvtest is an in-memory stand-in for uc-rendezvous's
// `/v1/web-pairings` routes, for tests.
package rvtest

import (
	"encoding/json"
	"fmt"
	"net/http"
	"net/http/httptest"
	"sync"
	"time"
)

type entry struct {
	ticket   string
	expires  time.Time
	consumed bool
}

// Server follows the route contract: create, resolve (repeatable), consume;
// 404 unknown or expired, 409 consumed.
type Server struct {
	*httptest.Server
	TTL time.Duration

	mu       sync.Mutex
	codes    map[string]*entry
	next     int
	Consumed []string
	// Fail, when set, answers every request with this status.
	Fail int
}

func New() *Server {
	s := &Server{TTL: 5 * time.Minute, codes: map[string]*entry{}, next: 482913}
	mux := http.NewServeMux()
	mux.HandleFunc("POST /v1/web-pairings", s.create)
	mux.HandleFunc("POST /v1/web-pairings/resolve", s.resolve)
	mux.HandleFunc("POST /v1/web-pairings/consume", s.consume)
	s.Server = httptest.NewServer(mux)
	return s
}

func (s *Server) body(w http.ResponseWriter, r *http.Request, v any) bool {
	s.mu.Lock()
	fail := s.Fail
	s.mu.Unlock()
	if fail != 0 {
		w.WriteHeader(fail)
		return false
	}
	if json.NewDecoder(r.Body).Decode(v) != nil {
		w.WriteHeader(400)
		return false
	}
	return true
}

func (s *Server) create(w http.ResponseWriter, r *http.Request) {
	var b struct{ Ticket string }
	if !s.body(w, r, &b) {
		return
	}
	s.mu.Lock()
	n := s.next
	s.next++
	code := fmt.Sprintf("%03d-%03d", n/1000%1000, n%1000)
	exp := time.Now().Add(s.TTL)
	s.codes[code] = &entry{ticket: b.Ticket, expires: exp}
	s.mu.Unlock()
	_ = json.NewEncoder(w).Encode(map[string]any{"code": code, "expiresAtMs": exp.UnixMilli()})
}

func (s *Server) lookup(w http.ResponseWriter, code string) *entry {
	e := s.codes[code]
	switch {
	case e == nil || time.Now().After(e.expires):
		w.WriteHeader(404)
		return nil
	case e.consumed:
		w.WriteHeader(409)
		return nil
	}
	return e
}

func (s *Server) resolve(w http.ResponseWriter, r *http.Request) {
	var b struct{ Code string }
	if !s.body(w, r, &b) {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.lookup(w, b.Code); e != nil {
		_ = json.NewEncoder(w).Encode(map[string]any{"ticket": e.ticket, "expiresAtMs": e.expires.UnixMilli()})
	}
}

func (s *Server) consume(w http.ResponseWriter, r *http.Request) {
	var b struct{ Code string }
	if !s.body(w, r, &b) {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.lookup(w, b.Code); e != nil {
		e.consumed = true
		s.Consumed = append(s.Consumed, b.Code)
		_ = json.NewEncoder(w).Encode(map[string]bool{"ok": true})
	}
}

// Put registers a raw ticket under code, e.g. a website transfer ticket.
func (s *Server) Put(code, ticket string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.codes[code] = &entry{ticket: ticket, expires: time.Now().Add(s.TTL)}
}

// SetFail makes every request answer status (0 to stop).
func (s *Server) SetFail(status int) {
	s.mu.Lock()
	s.Fail = status
	s.mu.Unlock()
}

// WasConsumed reports whether code was consumed.
func (s *Server) WasConsumed(code string) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, c := range s.Consumed {
		if c == code {
			return true
		}
	}
	return false
}
