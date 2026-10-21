package main

import (
	"bytes"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

const effectsPolicyVersion = 2

// Only non-content UI enums and format/policy versions may be persisted here.
type storedVisualEffects struct {
	SchemaVersion int           `json:"schemaVersion"`
	PolicyVersion int           `json:"policyVersion"`
	Mode          EffectsMode   `json:"mode"`
	NextAuto      *AutoResult   `json:"nextAuto"`
	EvidenceClass evidenceClass `json:"evidenceClass"`
}

type effectsStorage struct {
	path     string
	writable bool
}

func newEffectsStorage() effectsStorage {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return effectsStorage{}
	}
	return effectsStorage{path: filepath.Join(root, "visual-effects.json")}
}

func defaultStoredEffects() storedVisualEffects {
	return storedVisualEffects{SchemaVersion: 1, PolicyVersion: effectsPolicyVersion, Mode: EffectsModeAuto, EvidenceClass: evidenceUnknown}
}

func (s effectsStorage) load() (storedVisualEffects, bool) {
	defaults := defaultStoredEffects()
	if s.path == "" {
		return defaults, false
	}
	f, err := os.Open(s.path)
	if errors.Is(err, os.ErrNotExist) {
		return defaults, true
	}
	if err != nil {
		return defaults, false
	}
	defer f.Close()
	raw, err := io.ReadAll(io.LimitReader(f, 4097))
	if err != nil || len(raw) > 4096 || len(bytes.TrimSpace(raw)) == 0 || bytes.TrimSpace(raw)[0] != '{' {
		return defaults, false
	}
	value := defaults
	value.PolicyVersion = 0 // Missing policy versions must invalidate old automatic evidence.
	if json.Unmarshal(raw, &value) != nil || value.SchemaVersion != 1 ||
		(value.Mode != EffectsModeAuto && value.Mode != EffectsModeEffects && value.Mode != EffectsModeSmooth) ||
		(value.EvidenceClass != evidenceCapable && value.EvidenceClass != evidenceConstrained && value.EvidenceClass != evidenceUnknown) ||
		(value.NextAuto != nil && *value.NextAuto != AutoResultEffects && *value.NextAuto != AutoResultSmooth) {
		return defaults, false
	}
	return value, true
}

func (s effectsStorage) save(value storedVisualEffects) error {
	if !s.writable {
		return errors.New("visual effects configuration is read-only")
	}
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	dir := filepath.Dir(s.path)
	if err := os.MkdirAll(dir, 0700); err != nil {
		return err
	}
	f, err := os.CreateTemp(dir, ".visual-effects-*")
	if err != nil {
		return err
	}
	defer os.Remove(f.Name())
	if _, err := f.Write(raw); err != nil {
		f.Close()
		return err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return err
	}
	if err := f.Close(); err != nil {
		return err
	}
	return replaceEffectsFile(f.Name(), s.path)
}
