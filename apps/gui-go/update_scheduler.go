package main

import (
	"context"
	"encoding/json"
	"errors"
	"log"
	"math/rand/v2"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// schedulerTiming mirrors the cadence of the Tauri update scheduler
// (crates/uc-tauri/src/update_scheduler): setup polling every 30s, a 6h ± 15min
// cadence after a successful or idle iteration and a fixed 30min retry after a failure.
type schedulerTiming struct {
	setupPoll time.Duration
	success   time.Duration
	jitter    time.Duration
	failure   time.Duration
}

var defaultSchedulerTiming = schedulerTiming{
	setupPoll: 30 * time.Second,
	success:   6 * time.Hour,
	jitter:    15 * time.Minute,
	failure:   30 * time.Minute,
}

// Minimum gap between two scheduler-triggered prompt windows, per channel family.
const (
	stablePromptCooldown     = 72 * time.Hour
	prereleasePromptCooldown = 24 * time.Hour
)

func (t schedulerTiming) next(ok bool) time.Duration {
	if !ok {
		return t.failure
	}
	if t.jitter <= 0 {
		return t.success
	}
	return t.success + time.Duration(rand.Int64N(int64(2*t.jitter)+1)) - t.jitter
}

// promptStore owns the three small JSON files the Tauri shell also uses to avoid
// nagging the user: skipped_version.json, last_notified_update.json and
// update_prompt_throttle.json. One lock serialises every read-modify-write.
type promptStore struct{ mu sync.Mutex }

func readJSONFile(name string, out any) {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return
	}
	if raw, err := os.ReadFile(filepath.Join(root, name)); err == nil {
		_ = json.Unmarshal(raw, out) // a corrupt file is treated as empty, like the Tauri shell
	}
}

func writeJSONFile(name string, value any) error {
	root, ok := apppaths.AppDataRoot()
	if !ok {
		return errors.New("data root unavailable")
	}
	raw, err := json.Marshal(value)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return err
	}
	path := filepath.Join(root, name)
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, raw, 0o600); err != nil {
		return err
	}
	return os.Rename(tmp, path)
}

func promptCooldown(channel update.Channel) time.Duration {
	if channel == update.Stable {
		return stablePromptCooldown
	}
	return prereleasePromptCooldown
}

type promptThrottle struct {
	LastPromptAt *int64 `json:"last_prompt_at"`
}

func (s *promptStore) allowsPrompt(channel update.Channel) bool {
	var t promptThrottle
	readJSONFile("update_prompt_throttle.json", &t)
	if t.LastPromptAt == nil {
		return true
	}
	elapsed := time.Now().Unix() - *t.LastPromptAt
	return elapsed < 0 || time.Duration(elapsed)*time.Second >= promptCooldown(channel)
}

func (s *promptStore) recordPrompt() {
	now := time.Now().Unix()
	if err := writeJSONFile("update_prompt_throttle.json", promptThrottle{LastPromptAt: &now}); err != nil {
		log.Printf("update scheduler: persist prompt throttle: %v", err)
	}
}

// shouldPrompt applies the skip list and the per-version dedup (and, for scheduled
// triggers, the cooldown) without recording anything.
func (s *promptStore) shouldPrompt(channel update.Channel, version string, scheduled bool) bool {
	skipped, notified := map[string]string{}, map[string]string{}
	readJSONFile("skipped_version.json", &skipped)
	readJSONFile("last_notified_update.json", &notified)
	if skipped[string(channel)] == version || notified[string(channel)] == version {
		return false
	}
	// The cooldown is checked after the dedup and never recorded into last_notified,
	// so the first prompt after it expires shows the newest version.
	return !scheduled || s.allowsPrompt(channel)
}

func (s *promptStore) recordNotified(channel update.Channel, version string) {
	notified := map[string]string{}
	readJSONFile("last_notified_update.json", &notified)
	notified[string(channel)] = version
	if err := writeJSONFile("last_notified_update.json", notified); err != nil {
		log.Printf("update scheduler: persist last notified version: %v", err)
	}
	s.recordPrompt()
}

// notifyIfNew opens the updater window for a release the user has not been told
// about yet and records it only once the window is up. It reports whether it did.
func (h *HostService) notifyIfNew(channel update.Channel, version string) bool {
	s := &h.prompts
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.shouldPrompt(channel, version, true) {
		return false
	}
	h.openUpdater(false)
	s.recordNotified(channel, version)
	return true
}

// openReadyFallback re-opens the window when an auto-download reached "ready" but
// the dedup kept it closed because an earlier process already announced the version.
func (h *HostService) openReadyFallback(channel update.Channel) {
	s := &h.prompts
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.allowsPrompt(channel) {
		return
	}
	h.openUpdater(false)
	s.recordPrompt()
}

// setupComplete reports whether first-run setup has finished; before that the
// scheduler must not prompt.
func (h *HostService) setupComplete(ctx context.Context) (bool, error) {
	var state struct {
		HasCompleted bool `json:"hasCompleted"`
	}
	if err := h.client.Get(ctx, "/v2/setup/state", &state); err != nil {
		return false, err
	}
	return state.HasCompleted, nil
}

// sleepCtx waits for d or the context, reporting whether the full wait elapsed.
func sleepCtx(ctx context.Context, d time.Duration) bool {
	t := time.NewTimer(d)
	defer t.Stop()
	select {
	case <-ctx.Done():
		return false
	case <-t.C:
		return true
	}
}

// runUpdateScheduler is the background periodic update check. It waits for setup
// to complete, checks once immediately, then repeats on the success/failure cadence
// until ctx is cancelled. Native wake sources (App Nap exit) are not wired yet.
func (h *HostService) runUpdateScheduler(ctx context.Context) {
	timing := schedulerTimingOverride(defaultSchedulerTiming)
	for {
		done, err := h.setupComplete(ctx)
		if err == nil && done {
			break
		}
		if !sleepCtx(ctx, timing.setupPoll) {
			return
		}
	}
	for {
		if !sleepCtx(ctx, timing.next(h.scheduledCheck(ctx))) {
			return
		}
	}
}

// scheduledCheck runs one iteration and reports whether it counts as a success.
// A disabled auto-check is an idle success so toggling it on has no retry penalty.
func (h *HostService) scheduledCheck(ctx context.Context) bool {
	var settings struct {
		General struct {
			AutoCheckUpdate    bool `json:"autoCheckUpdate"`
			AutoDownloadUpdate bool `json:"autoDownloadUpdate"`
		} `json:"general"`
	}
	if err := h.client.Get(ctx, "/settings", &settings); err != nil {
		log.Printf("update scheduler: load settings: %v", err)
		return false
	}
	if !settings.General.AutoCheckUpdate {
		return true
	}
	checkCtx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	channel := h.resolveChannel(checkCtx, nil)
	meta, err := h.checkForUpdate(checkCtx, nil)
	if err != nil {
		log.Printf("update scheduler: check failed: %v", err)
		return false
	}
	release, _ := meta.(map[string]any)
	version, _ := release["version"].(string)
	if version == "" {
		return true
	}
	opened := h.notifyIfNew(channel, version)
	if settings.General.AutoDownloadUpdate {
		// In-place install is supported on macOS, the only host this build targets.
		if err := h.downloadUpdate(ctx); err != nil {
			log.Printf("update scheduler: auto-download failed: %v", err)
		} else if !opened {
			h.openReadyFallback(channel)
		}
	}
	return true
}
