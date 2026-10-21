//go:build e2e

package main

import (
	"fmt"
	"path/filepath"
	"time"
)

// ConfigureVisualEffects injects capability evidence at the native probe seam.
// It exercises real commands/storage in a WebView, not the machine's GPU.
func (s *EvidenceService) ConfigureVisualEffects(platform string, class string) error {
	if platform != "linux" && platform != "darwin" && platform != "windows" {
		return fmt.Errorf("invalid fixture platform")
	}
	if class != string(evidenceUnknown) && class != string(evidenceConstrained) && class != string(evidenceCapable) {
		return fmt.Errorf("invalid fixture class")
	}
	v := s.host.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	v.platform = platform
	v.initialize(evidenceClass(class))
	v.started = time.Now().Add(-11 * time.Second)
	v.invalidateSamples()
	s.host.publishVisualEffects(v.snapshot())
	return nil
}

// AdvanceVisualEffectsInterval avoids a 30-second wall-clock delay between
// independent permits. Production sampling still uses time.Now exclusively.
func (s *EvidenceService) AdvanceVisualEffectsInterval() {
	v := s.host.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	v.lastSample = time.Now().Add(-31 * time.Second)
}

// SetVisualEffectsWriteFailure makes the actual atomic replacement target an
// existing directory. The OS rejects the write without damaging the saved file.
func (s *EvidenceService) SetVisualEffectsWriteFailure(enabled bool) {
	v := s.host.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	v.storage.path = newEffectsStorage().path
	if enabled {
		v.storage.path = filepath.Dir(v.storage.path)
	}
}
