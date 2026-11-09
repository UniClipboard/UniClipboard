package lifecycle

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"sync"
)

// LoginRegistration keeps user intent separate from the operating system's state.
// A receipt prevents a later launch from undoing an external system disable.
type LoginRegistration struct {
	mu         sync.Mutex
	Path       string
	Enabled    func() bool
	SetEnabled func(bool) error
}

// Apply changes registration only for new intent or an explicit retry.
// The returned boolean is always the actual system state.
func (r *LoginRegistration) Apply(requested, retry bool) (bool, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	var previous struct {
		Requested bool `json:"requested"`
	}
	raw, err := os.ReadFile(r.Path)
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return r.Enabled(), err
	}
	recorded := err == nil
	if recorded {
		if err := json.Unmarshal(raw, &previous); err != nil && !retry {
			return r.Enabled(), err
		}
	}
	if !retry && recorded && previous.Requested == requested {
		return r.Enabled(), nil
	}
	actual := r.Enabled()
	if actual != requested || retry || (recorded && previous.Requested != requested) {
		if err := r.SetEnabled(requested); err != nil {
			return r.Enabled(), err
		}
		actual = r.Enabled()
		if actual != requested {
			return actual, errors.New("system login registration requires approval")
		}
	}
	raw, err = json.Marshal(struct {
		Requested bool `json:"requested"`
	}{requested})
	if err != nil {
		return actual, err
	}
	file, err := os.CreateTemp(filepath.Dir(r.Path), ".login-registration-*")
	if err != nil {
		return actual, err
	}
	defer func() { _ = os.Remove(file.Name()) }()
	if _, err = file.Write(raw); err == nil {
		err = file.Sync()
	}
	if closeErr := file.Close(); err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Rename(file.Name(), r.Path)
	}
	return actual, err
}
