package main

import (
	"sync"
	"sync/atomic"
)

// exitIntent has one irreversible decision per host. In particular, UI reaping
// cannot turn a full quit into a daemon-preserving exit, and concurrent mode
// changes cannot spawn multiple replacement hosts.
type exitIntent struct {
	mu         sync.Mutex
	quitting   atomic.Bool
	keepDaemon atomic.Bool
}

func (e *exitIntent) begin(keep bool) bool {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.quitting.Load() {
		return false
	}
	e.keepDaemon.Store(keep)
	e.quitting.Store(true)
	return true
}

func (e *exitIntent) restart(start func() error) (bool, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	if e.quitting.Load() {
		return false, nil
	}
	if err := start(); err != nil {
		return false, err
	}
	e.keepDaemon.Store(true)
	e.quitting.Store(true)
	return true, nil
}
