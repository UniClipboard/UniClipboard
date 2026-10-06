//go:build e2e

package main

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"sync/atomic"
	"time"
)

// Scripted keyboard for the modifier double-tap monitor (e2e build only). UC_GUI_GO_E2E_SCRIPTED_KEYS=1 replaces the
// platform snapshot with a state the control file drives, so the real monitor goroutine (20 ms poll, detector,
// toggle request) runs against a keyboard sequence with exact timing and without any keyboard event. It covers the
// Uni logic; it does not cover the Win32 GetAsyncKeyState read, which only a Windows host can.

var scripted struct {
	selected, other atomic.Bool
	triggers        atomic.Int64
}

func scriptedKeyState() bool { return os.Getenv("UC_GUI_GO_E2E_SCRIPTED_KEYS") == "1" }

type scriptedKeys struct{}

func (scriptedKeys) snapshot(string) (bool, bool) {
	return scripted.selected.Load(), scripted.other.Load()
}

func modifierKeyStateFactory() func() (modifierKeyState, error) {
	if scriptedKeyState() {
		return func() (modifierKeyState, error) { return scriptedKeys{}, nil }
	}
	return newPlatformKeyState
}

func e2eModifierTriggered() { scripted.triggers.Add(1) }

// runModifierScript plays "<ms>:<selected>:<other>" steps (e.g. "50:0:0 30:1:0"): each sets the scripted key state
// and holds it for that long. A state of 1 means down.
func runModifierScript(steps string) error {
	for _, step := range strings.Fields(steps) {
		parts := strings.Split(step, ":")
		if len(parts) != 3 {
			return fmt.Errorf("bad step %q", step)
		}
		ms, err := strconv.Atoi(parts[0])
		if err != nil {
			return err
		}
		scripted.selected.Store(parts[1] == "1")
		scripted.other.Store(parts[2] == "1")
		time.Sleep(time.Duration(ms) * time.Millisecond)
	}
	return nil
}

// controlModifier handles `modifier-script <label> <steps>` and `modifier-state <label>`.
func (s *EvidenceService) controlModifier(verb, arg string) {
	h := s.host
	label, rest, _ := strings.Cut(arg, " ")
	switch verb {
	case "modifier-script":
		before := scripted.triggers.Load()
		err := runModifierScript(rest)
		time.Sleep(3 * modifierPollInterval) // let the poll see the last state
		detail := map[string]any{"triggers": scripted.triggers.Load() - before, "monitor": h.modifierMonitor().Current()}
		if err != nil {
			detail["error"] = err.Error()
		}
		_ = s.write(Step{Window: quickPanelWindowName, Step: "modifier-script-" + label, OK: err == nil, Detail: detail})
	case "modifier-state":
		_ = s.write(Step{Window: quickPanelWindowName, Step: "modifier-state-" + label, OK: true, Detail: map[string]any{
			"monitor": h.modifierMonitor().Current(), "triggers": scripted.triggers.Load(), "lastShown": h.panel.lastShown.Load()}})
	}
}
