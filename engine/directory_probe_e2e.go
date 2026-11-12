//go:build unic_e2e

package engine

import (
	"context"
	"sync"
)

type directoryProbe struct {
	holdStage        string
	mu               sync.Mutex
	scans, snapshots uint64
	hold             chan struct{}
}

func (p *directoryProbe) observe(ctx context.Context, stage string) error {
	p.mu.Lock()
	if stage == "scan" {
		p.scans++
	} else if stage == "snapshot" {
		p.snapshots++
	}
	hold := p.hold
	holdStage := p.holdStage
	p.mu.Unlock()
	if stage == holdStage && hold != nil {
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-hold:
		}
	}
	return ctx.Err()
}
func (e *Engine) DirectoryProbeCounts() (uint64, uint64) {
	p := &e.directoryProbe
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.scans, p.snapshots
}
func (e *Engine) HoldDirectoryPreparation(hold bool) { e.HoldDirectoryStage(hold, "scan") }
func (e *Engine) HoldDirectoryStage(hold bool, stage string) {
	p := &e.directoryProbe
	p.mu.Lock()
	defer p.mu.Unlock()
	if hold && p.hold == nil {
		p.hold = make(chan struct{})
		p.holdStage = stage
	}
	if !hold && p.hold != nil {
		close(p.hold)
		p.hold = nil
	}
}
