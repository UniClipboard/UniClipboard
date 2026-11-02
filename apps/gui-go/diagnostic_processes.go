package main

import (
	"context"
	"log"
	"time"
)

// registerUIDiagnostics uses the child owned by this host, never process-name discovery.
func (h *HostService) registerUIDiagnostics(ctx context.Context) {
	if h.ui == nil || !h.ui.mu.TryLock() {
		return
	}
	child := h.ui.child
	alive := child != nil && child.Process != nil
	if alive {
		select {
		case <-h.ui.exited:
			alive = false
		default:
		}
	}
	var pid uint32
	if alive {
		pid = uint32(child.Process.Pid)
	}
	h.ui.mu.Unlock()
	if alive {
		h.registerDiagnosticPID(ctx, pid, "gui")
	}
}

func (h *HostService) registerDiagnosticPID(ctx context.Context, pid uint32, role string) {
	client := h.daemon()
	if client == nil {
		return
	}
	ctx, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	if err := client.RegisterOwnedProcess(ctx, pid, role); err != nil {
		log.Printf("diagnostic process registration unavailable role=%s", role)
	}
}
