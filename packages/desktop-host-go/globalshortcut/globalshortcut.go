// Package globalshortcut keeps one global shortcut's desired binding (what the settings say) apart from the binding
// its executor actually holds, and decides when the executor must change (ADR-013 section 10). It has no GUI
// dependency: the Go host plugs in the executor (the native helper or the WebView panel's Wails binder) and calls it
// from its commands and from the settings watcher.
package globalshortcut

import (
	"errors"
	"log"
	"slices"
	"sync"
)

// ErrUnknown marks an apply whose outcome the executor cannot report (the helper did not answer). The bindings it
// holds are then unknown.
var ErrUnknown = errors.New("the shortcut state is unknown")

// Executor binds a set of physical-key shortcuts with the OS. An empty set releases every binding. Apply returns the
// bindings the executor holds afterwards, also after a refusal (a transactional executor still holds the previous
// ones). An error wrapping ErrUnknown means the held bindings are unknown and the returned slice is meaningless.
type Executor interface {
	Apply(bindings []string) (held []string, err error)
}

// State is what the user is told about the shortcut.
type State string

const (
	// StateActive: the executor reported holding the desired binding.
	StateActive State = "active"
	// StateNotActive: the desired binding is saved but the executor holds something else (Active).
	StateNotActive State = "not_active"
	// StateUnknown: the executor could not report what it holds.
	StateUnknown State = "unknown"
	// StateInactive: nothing is bound on purpose (the panel is disabled, or no executor runs).
	StateInactive State = "inactive"
)

// Status is the shortcut's apply status. It lives in memory only. Active is empty when the state is unknown.
type Status struct {
	State   State
	Desired []string
	Active  []string
	Error   string
}

// Applier owns the desired and the active binding of one executor. Every method may be called from any goroutine;
// calls are serialized.
type Applier struct {
	mu          sync.Mutex
	exec        Executor
	known       bool // desired has been set once
	desired     []string
	active      []string // as last reported by the executor
	activeKnown bool
	lastGood    []string // the last binding the executor confirmed holding; the fallback after a relaunch
	err         string
	onChange    func(Status)
}

// New returns an applier for exec (nil: no executor in this process). onChange, if set, receives every status
// change; it is called with the applier's lock held and must not call back into it.
func New(exec Executor, onChange func(Status)) *Applier {
	return &Applier{exec: exec, onChange: onChange, activeKnown: true}
}

// Status returns the current status.
func (a *Applier) Status() Status {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.statusLocked()
}

// Init records and applies the binding read at startup. A failure is recorded in the status, not returned.
func (a *Applier) Init(target []string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.known = true
	a.desired = slices.Clone(target)
	a.applyLocked(target)
	a.notifyLocked()
}

// Record sets the desired binding without applying it, for an executor that is about to start or has just stopped
// (the helper is launched or ended by the enabled switch; Relaunched applies on launch).
func (a *Applier) Record(target []string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.known = true
	a.desired = slices.Clone(target)
	a.err = ""
	a.notifyLocked()
}

// Save is the GUI path: the executor takes target first, so a refused binding is reported before anything is saved;
// persist runs only after that, and a failed persist asks the executor for the previous binding again. The status
// always shows what the executor reported, so a failed rollback is visible. Desired and active are recorded before
// the lock is released, so the settings event of this very save finds nothing to do.
func (a *Applier) Save(target []string, persist func() error) error {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.exec == nil || (a.known && a.settledLocked() && slices.Equal(target, a.desired)) {
		if err := persist(); err != nil {
			return err
		}
		a.known = true
		a.desired = slices.Clone(target)
		a.notifyLocked()
		return nil
	}
	previous := slices.Clone(a.active) // the rollback puts back what the executor held, not what was desired
	if !a.activeKnown {
		previous = slices.Clone(a.desired)
	}
	if err := a.applyLocked(target); err != nil {
		a.notifyLocked() // the refusal or the unknown outcome is part of the status; desired is unchanged
		return err
	}
	if err := persist(); err != nil {
		if rollback := a.applyLocked(previous); rollback != nil {
			log.Printf("global shortcut: could not restore the previous binding after the save failed: %v", rollback)
		}
		a.notifyLocked()
		return err
	}
	a.known = true
	a.desired = slices.Clone(target)
	a.notifyLocked()
	return nil
}

// Reconcile is the path of every settings read (an external write, a reconnect, the host's own write coming back).
// A target equal to the desired binding needs nothing while the executor's state is known, which keeps own writes and
// re-reads from registering again. Otherwise confirm reads the settings again under the lock, so a read that started
// before the host's own save cannot bring an old value back. A refused binding stays desired (it is saved) and the
// executor keeps what it held. While the state is unknown every read sends the desired binding again: replacing a
// binding with itself registers nothing, and the answer gives the actual state.
func (a *Applier) Reconcile(target []string, confirm func() ([]string, error)) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.known && a.activeKnown && slices.Equal(target, a.desired) {
		return
	}
	fresh, err := confirm()
	if err != nil {
		log.Printf("global shortcut: settings changed but could not be read again: %v", err)
		return
	}
	if a.known && a.activeKnown && slices.Equal(fresh, a.desired) {
		return
	}
	a.known = true
	a.desired = slices.Clone(fresh)
	_ = a.applyLocked(fresh)
	a.notifyLocked()
}

// Relaunched is called after the executor started afresh holding nothing (the helper process was launched). The
// desired binding is applied; when it is refused the last binding an executor confirmed holding is tried, so a crash
// does not lose a working binding. A new process may still be starting when the first command times out, so an unknown
// outcome is asked once more.
func (a *Applier) Relaunched() {
	a.mu.Lock()
	defer a.mu.Unlock()
	previous := slices.Clone(a.lastGood)
	a.active, a.activeKnown = nil, true
	if a.known && a.exec != nil {
		err := a.applyLocked(a.desired)
		if errors.Is(err, ErrUnknown) {
			err = a.applyLocked(a.desired)
		}
		if err != nil && len(previous) > 0 && !slices.Equal(previous, a.desired) && a.activeKnown && len(a.active) == 0 {
			refused := a.err
			if a.applyLocked(previous) == nil {
				a.err = refused // the desired binding is still refused; only the fallback took
			}
		}
	}
	a.notifyLocked()
}

// Stopped records that the executor holds nothing any more (the helper process ended on purpose).
func (a *Applier) Stopped() {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.active, a.activeKnown = nil, true
	a.notifyLocked()
}

// applyLocked asks the executor for target and records what it reports holding. err is recorded for the status:
// cleared on success, kept on a refusal or an unknown outcome.
func (a *Applier) applyLocked(target []string) error {
	if a.exec == nil {
		return nil
	}
	held, err := a.exec.Apply(target)
	switch {
	case errors.Is(err, ErrUnknown):
		a.active, a.activeKnown = nil, false
	default:
		a.active, a.activeKnown = slices.Clone(held), true
		if len(held) > 0 {
			a.lastGood = slices.Clone(held)
		}
	}
	if err != nil {
		a.err = err.Error()
		log.Printf("global shortcut: the executor did not take the binding: %v", err)
		return err
	}
	a.err = ""
	return nil
}

// settledLocked: the executor's state is known and it holds the desired binding.
func (a *Applier) settledLocked() bool {
	return a.activeKnown && a.err == "" && slices.Equal(a.desired, a.active)
}

func (a *Applier) statusLocked() Status {
	s := Status{Desired: slices.Clone(a.desired), Error: a.err}
	if a.activeKnown {
		s.Active = slices.Clone(a.active)
	}
	switch {
	case a.exec == nil || !a.known || len(a.desired) == 0:
		s.State = StateInactive
	case !a.activeKnown:
		s.State = StateUnknown
	case slices.Equal(a.desired, a.active):
		s.State = StateActive
		s.Error = ""
	default:
		s.State = StateNotActive
		if s.Error == "" {
			s.Error = "the shortcut is not registered"
		}
	}
	return s
}

func (a *Applier) notifyLocked() {
	if a.onChange != nil {
		a.onChange(a.statusLocked())
	}
}
