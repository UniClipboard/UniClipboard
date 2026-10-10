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
	"sync/atomic"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/apppaths"
)

// schedulerTiming mirrors the cadence of the Tauri update scheduler
// (crates/uc-tauri/src/update_scheduler): setup polling every 30s, a 6h ± 15min
// cadence after a successful or idle iteration and a fixed 30min retry after a failure; a system wake runs an
// extra check only when the last one is older than wakeMinRecheck.
type schedulerTiming struct {
	setupPoll time.Duration
	success   time.Duration
	jitter    time.Duration
	failure   time.Duration
	// wakeMinRecheck is how long after the last check (from any source) a system wake may trigger another
	// one: below it the check is skipped so a short sleep or a burst of resume events never hits the feed twice.
	wakeMinRecheck time.Duration
	// activityInterval is the period of the macOS background activity that fires while App Nap suspends the
	// timers below; like the Tauri shell it equals the success cadence.
	activityInterval time.Duration
}

var defaultSchedulerTiming = schedulerTiming{
	setupPoll:      30 * time.Second,
	success:        6 * time.Hour,
	jitter:         15 * time.Minute,
	failure:        30 * time.Minute,
	wakeMinRecheck: time.Hour,

	activityInterval: 6 * time.Hour,
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

// notifyIfNew opens the updater window for a release the user has not been told about yet, reports the
// announcement and records it once the window is up. A scheduled trigger also honours the prompt cooldown; a
// manual one does not. It reports whether it opened the window.
func (h *HostService) notifyIfNew(channel update.Channel, version string, scheduled bool) bool {
	s := &h.prompts
	s.mu.Lock()
	defer s.mu.Unlock()
	if !s.shouldPrompt(channel, version, scheduled) {
		return false
	}
	h.openUpdater(false)
	h.reportNotification(version, deliverySent)
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
	if err := h.daemon().Get(ctx, "/v2/setup/state", &state); err != nil {
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

// lastCheckAt is the wall-clock time of the last finished check from any source, the guard that keeps a
// wake from re-checking right after a scheduled or manual check. It starts at "now" so the first wake after
// launch does not duplicate the immediate startup check. Wall clock, not the monotonic timers, because the
// monotonic clock does not advance while the machine sleeps.
type lastCheckAt struct{ unix atomic.Int64 }

func (l *lastCheckAt) recordNow() { l.unix.Store(time.Now().Unix()) }

// since is the time since the last check, clamped at zero so a clock set backwards cannot pass the guard.
func (l *lastCheckAt) since() time.Duration {
	return max(0, time.Duration(time.Now().Unix()-l.unix.Load())*time.Second)
}

// Where a scheduler wake comes from; the log names it so a check can be traced to its trigger.
const (
	wakeSystemResume       = "system-did-wake"     // Wails Common.SystemDidWake: the machine woke from sleep
	wakeBackgroundActivity = "background-activity" // macOS NSBackgroundActivityScheduler: the system ran the activity, App Nap or not
)

// signalWake queues one pending wake (a burst of events, from any source, collapses into one) and never blocks
// the caller: it runs on the Wails event loop and on a system queue.
func (h *HostService) signalWake(source string) {
	if source == wakeSystemResume {
		log.Printf("update scheduler: system wake")
	}
	select {
	case h.wake <- source:
	default:
	}
}

// runUpdateScheduler is the background periodic update check. It waits for setup to complete, checks once
// immediately, then repeats on the success/failure cadence until ctx is cancelled. A wake (signalWake: system
// resume or the macOS background activity) checks early only when the last check is older than
// wakeMinRecheck; a skipped wake leaves the cadence timer untouched. Wake events during the setup wait are
// ignored, as in the Tauri scheduler.
func (h *HostService) runUpdateScheduler(ctx context.Context, timing schedulerTiming) {
	for {
		done, err := h.setupComplete(ctx)
		if err == nil && done {
			break
		}
		if !sleepCtx(ctx, timing.setupPoll) {
			return
		}
	}
	ok := h.scheduledCheck(ctx)
	timer := time.NewTimer(timing.next(ok))
	defer timer.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-timer.C:
			ok = h.scheduledCheck(ctx)
		case source := <-h.wake:
			since := h.lastCheck.since()
			if since < timing.wakeMinRecheck {
				log.Printf("update scheduler: wake skipped, last check %s ago (%s)", since.Round(time.Second), source)
				continue
			}
			log.Printf("update scheduler: wake after %s, checking (%s)", since.Round(time.Second), source)
			ok = h.scheduledCheck(ctx)
			if !timer.Stop() {
				select {
				case <-timer.C:
				default:
				}
			}
		}
		timer.Reset(timing.next(ok))
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
	if err := h.daemon().Get(ctx, "/settings", &settings); err != nil {
		log.Printf("update scheduler: load settings: %v", err)
		return false
	}
	if !settings.General.AutoCheckUpdate {
		return true
	}
	checkCtx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	channel := h.resolveChannel(checkCtx, nil)
	meta, err := h.lookupUpdate(checkCtx, nil)
	if err != nil {
		log.Printf("update scheduler: check failed: %v", err)
	}
	// Side effects first, the check event last: that is the order the Tauri scheduler reports them in.
	if meta != nil && meta.Version != "" {
		opened := h.notifyIfNew(channel, meta.Version, true)
		if settings.General.AutoDownloadUpdate {
			// In-place install is supported on macOS, the only host this build targets.
			// A refused download (already downloaded or running) neither reports nor re-opens the window.
			switch err := h.downloadUpdateReported(ctx); {
			case err == nil:
				if !opened {
					h.openReadyFallback(channel)
				}
			case classifyDownload(err) != downloadPrecondition:
				log.Printf("update scheduler: auto-download failed: %v", err)
			}
		}
	}
	h.reportCheck(checkSourceScheduled, meta != nil, err)
	return err == nil
}
