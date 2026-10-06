package main

import "sync"

// toggleState decides when a request to toggle the quick panel may act. Requests that arrive before the
// settings are known or before the panel page has loaded are not lost and not applied twice: they only flip a
// pending flag, so N early presses end up with the same result as N presses after startup (parity). It is the
// Go form of QuickPanelToggleController in crates/uc-tauri/src/quick_panel/mod.rs.
type toggleState struct {
	mu         sync.Mutex
	configured bool // the persisted enabled setting is known
	enabled    bool
	ready      bool // the panel page has finished loading (mark_quick_panel_ready)
	pending    bool
}

// request reports whether the toggle may run now; otherwise it is parked as pending parity.
func (s *toggleState) request() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.configured && !s.enabled {
		return false
	}
	if !s.configured || !s.ready {
		s.pending = !s.pending
		return false
	}
	return true
}

// markReady records that the panel page is loaded and reports whether a parked toggle should run now.
func (s *toggleState) markReady() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.ready = true
	if s.enabled {
		return takeFlag(&s.pending)
	}
	return false
}

// configure records the persisted setting at startup and reports whether a parked toggle should run now.
func (s *toggleState) configure(enabled bool) bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.configured, s.enabled = true, enabled
	if !enabled {
		s.pending = false
		return false
	}
	if s.ready {
		return takeFlag(&s.pending)
	}
	return false
}

func (s *toggleState) setEnabled(enabled bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.configured, s.enabled = true, enabled
	if !enabled {
		s.pending = false
	}
}

func (s *toggleState) isReady() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.ready
}

func takeFlag(flag *bool) bool {
	was := *flag
	*flag = false
	return was
}
