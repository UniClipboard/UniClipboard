package main

import (
	"fmt"
	"sync"
	"time"
)

// The modifier double-tap trigger of the WebView quick panel: two standalone taps of Alt, Control or Meta open the
// panel. It is the Go form of crates/uc-desktop/src/modifier_double_tap.rs (detector) and
// modifier_double_tap_monitor.rs (worker), with the same constants. A global modifier-only gesture cannot be
// registered as a hot key, so it is detected by polling a keyboard snapshot; Wails has no API for it.

const (
	// modifierPollInterval keeps recognition latency near one display frame while bounding the active cost at 50
	// keyboard snapshots per second; the poll only runs while a modifier is selected.
	modifierPollInterval = 20 * time.Millisecond
	// doubleTapWindow is how long the second tap may follow the release of the first.
	doubleTapWindow = 400 * time.Millisecond
	// modifierWorkerStopTimeout bounds how long a shutdown waits for the worker, so a trigger callback that is
	// stuck behind the UI thread cannot hang the exit.
	modifierWorkerStopTimeout = 2 * time.Second
)

// modifierKeyState supplies one keyboard snapshot: whether the selected modifier is down and whether any other key
// (or modifier) is down.
type modifierKeyState interface {
	snapshot(modifier string) (selectedDown, otherDown bool)
}

// doubleTapDetector recognizes two standalone taps of one modifier from snapshots. Any other key invalidates the
// tap in progress and clears a pending first tap, so ordinary shortcuts cannot complete the gesture. It assumes the
// caller samples often enough to see every relevant transition: a full press and release between two samples is
// invisible.
type doubleTapDetector struct {
	window            time.Duration
	initialized       bool
	wasDown           bool
	activeTapInvalid  bool
	lastTapReleasedAt time.Time
	hasLastTap        bool
}

func newDoubleTapDetector() *doubleTapDetector { return &doubleTapDetector{window: doubleTapWindow} }

func (d *doubleTapDetector) observe(selectedDown, otherDown bool, now time.Time) bool {
	if !d.initialized {
		// The first sample only seeds the state: a modifier that is already held is not part of a tap.
		d.initialized = true
		d.wasDown = selectedDown
		d.activeTapInvalid = selectedDown || otherDown
		return false
	}
	if otherDown {
		d.hasLastTap = false
		if selectedDown {
			d.activeTapInvalid = true
		}
	}
	if selectedDown && !d.wasDown {
		d.activeTapInvalid = otherDown
	}
	triggered := false
	if !selectedDown && d.wasDown {
		validTap := !d.activeTapInvalid && !otherDown
		d.activeTapInvalid = false
		if validTap {
			isDouble := d.hasLastTap && now.Sub(d.lastTapReleasedAt) <= d.window
			d.hasLastTap = !isDouble
			if !isDouble {
				d.lastTapReleasedAt = now
			}
			triggered = isDouble
		}
	}
	d.wasDown = selectedDown
	return triggered
}

// modifierMonitor owns the polling worker. The worker exists only while a modifier is selected: disabling it or
// shutting down releases the keyboard backend and waits for the worker to exit.
type modifierMonitor struct {
	newKeyState func() (modifierKeyState, error)
	onTrigger   func()

	mu      sync.Mutex
	current string
	worker  *modifierWorker
}

type modifierWorker struct {
	commands chan string // the selected modifier; closed to stop
	done     chan struct{}
}

func newModifierMonitor(newKeyState func() (modifierKeyState, error), onTrigger func()) *modifierMonitor {
	return &modifierMonitor{newKeyState: newKeyState, onTrigger: onTrigger, current: "disabled"}
}

// Current is the modifier the worker is watching for, "disabled" when none.
func (m *modifierMonitor) Current() string {
	m.mu.Lock()
	defer m.mu.Unlock()
	return m.current
}

// Set selects the modifier to watch (alt, control or meta) or stops watching (disabled). Selecting a modifier
// starts the worker if needed and restarts the detection, so a half-finished tap of the previous choice is dropped.
func (m *modifierMonitor) Set(modifier string) error {
	switch modifier {
	case "disabled":
		m.Shutdown()
		return nil
	case "alt", "control", "meta":
	default:
		return fmt.Errorf("unknown modifier %q", modifier)
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.worker == nil {
		state, err := m.newKeyState()
		if err != nil {
			return err
		}
		m.worker = &modifierWorker{commands: make(chan string, 1), done: make(chan struct{})}
		go m.worker.run(state, m.onTrigger)
	}
	select {
	case m.worker.commands <- modifier:
	default: // the worker has not consumed the previous selection yet: replace it
		select {
		case <-m.worker.commands:
		default:
		}
		m.worker.commands <- modifier
	}
	m.current = modifier
	return nil
}

// Shutdown stops the worker and waits for it (bounded). It is idempotent.
func (m *modifierMonitor) Shutdown() {
	m.mu.Lock()
	m.current = "disabled"
	worker := m.worker
	m.worker = nil
	m.mu.Unlock()
	if worker == nil {
		return
	}
	close(worker.commands)
	select {
	case <-worker.done:
	case <-time.After(modifierWorkerStopTimeout):
	}
}

func (w *modifierWorker) run(state modifierKeyState, onTrigger func()) {
	defer close(w.done)
	selected := "disabled"
	detector := newDoubleTapDetector()
	ticker := time.NewTicker(modifierPollInterval)
	defer ticker.Stop()
	for {
		select {
		case modifier, ok := <-w.commands:
			if !ok {
				return
			}
			selected, detector = modifier, newDoubleTapDetector()
		case <-ticker.C:
			if selected == "disabled" {
				continue
			}
			selectedDown, otherDown := state.snapshot(selected)
			if detector.observe(selectedDown, otherDown, time.Now()) {
				onTrigger()
			}
		}
	}
}

// desiredLiveModifier is the modifier that should be watched right now (Tauri `desired_live_modifier`): the stored
// choice while the quick panel is enabled and the platform can observe the keyboard, otherwise none.
func desiredLiveModifier(panelEnabled, available bool, stored string) string {
	if panelEnabled && available {
		return stored
	}
	return "disabled"
}

// modifierDoubleTapSupported reports whether this build can watch the keyboard for the trigger: the platform
// backend, or the scripted source of the e2e build.
func modifierDoubleTapSupported() bool { return modifierDoubleTapAvailable || scriptedKeyState() }
