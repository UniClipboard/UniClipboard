package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"math"
	"runtime"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

const (
	effectsProbeTimeout   = 2 * time.Second
	effectsWarmup         = 10 * time.Second
	effectsSampleInterval = 30 * time.Second
	effectsSampleDuration = 2 * time.Second
	effectsMaxSamples     = 10
	effectsMinFrames      = 30
	effectsLongFrameMs    = 50
	effectsMaxStallMs     = 500
	effectsMinLongFrames  = 6
	effectsBadFraction    = 0.2
	effectsBadSamples     = 3
)

// visualEffects is the sole owner of device-local GUI effects. Native panel helpers
// neither participate in this session nor read its preferences.
type visualEffects struct {
	materialRevision int // Accessed only on the native UI thread.
	platform         string
	mu               sync.Mutex
	sessionID        string
	revision         int
	stored           storedVisualEffects
	storage          effectsStorage
	persistence      EffectsPersistence
	auto             AutoResult
	autoReason       EffectsReason
	ready            bool
	windows          map[uint]SystemMotion
	started          time.Time
	permit           *effectsPermit
	lastSample       time.Time
	samples          int
	badSamples       int
}

type effectsPermit struct {
	SamplePermit
	window  uint
	started time.Time
}

func newVisualEffects() *visualEffects {
	id := make([]byte, 16)
	if _, err := rand.Read(id); err != nil {
		panic(err)
	}
	storage := newEffectsStorage()
	stored, writable := storage.load()
	storage.writable = writable
	persistence := EffectsPersistenceSessionOnly
	// A validated loaded choice (or the default auto choice) already survives restart.
	// Pending hardware detection is not a persistence failure.
	if writable {
		persistence = EffectsPersistenceSaved
	}
	v := &visualEffects{platform: runtime.GOOS, sessionID: hex.EncodeToString(id), revision: 1,
		stored: stored, storage: storage, auto: AutoResultSmooth, autoReason: EffectsReasonUnknown,
		persistence: persistence, windows: make(map[uint]SystemMotion), started: time.Now()}
	return v
}

func (v *visualEffects) initialize(class evidenceClass) {
	if v.stored.PolicyVersion != effectsPolicyVersion || v.stored.EvidenceClass != class {
		v.stored.NextAuto = nil
	}
	v.stored.PolicyVersion, v.stored.EvidenceClass = effectsPolicyVersion, class
	v.auto, v.autoReason = AutoResultSmooth, EffectsReasonUnknown
	if v.platform == "linux" {
		v.stored.NextAuto = nil
		v.autoReason = EffectsReasonPlatformDefault
	} else if v.stored.NextAuto != nil {
		v.auto, v.autoReason = *v.stored.NextAuto, EffectsReasonRuntime
	} else if class == evidenceCapable {
		v.auto, v.autoReason = AutoResultEffects, EffectsReasonHardware
	} else if class == evidenceConstrained {
		v.autoReason = EffectsReasonHardware
	}
	v.ready = true
	v.revision++
	v.save()
}

func (v *visualEffects) save() {
	v.persistence = EffectsPersistenceSessionOnly
	if v.ready && v.storage.save(v.stored) == nil {
		v.persistence = EffectsPersistenceSaved
	}
}

func (v *visualEffects) systemMotion() SystemMotion {
	motion := SystemMotionAllow
	if len(v.windows) == 0 {
		return SystemMotionUnknown
	}
	for _, value := range v.windows {
		if value == SystemMotionReduce {
			return value
		}
		if value == SystemMotionUnknown {
			motion = value
		}
	}
	return motion
}

// snapshot and all transitions require mu. Pointers in snapshots never alias mutable state.
func (v *visualEffects) snapshot() EffectsSnapshot {
	motion := v.systemMotion()
	reduce := motion != SystemMotionAllow
	low := v.auto == AutoResultSmooth || reduce
	reason := v.autoReason
	switch v.stored.Mode {
	case EffectsModeEffects:
		low, reason = false, EffectsReasonManual
	case EffectsModeSmooth:
		reduce, low, reason = true, true, EffectsReasonManual
	case EffectsModeAuto:
		if reduce {
			reason = EffectsReasonSystem
		}
		reduce = reduce || low
	}
	var next *AutoResult
	if v.stored.NextAuto != nil {
		value := *v.stored.NextAuto
		next = &value
	}
	return EffectsSnapshot{SessionID: v.sessionID, Revision: v.revision, Mode: v.stored.Mode,
		AutoForSession: v.auto, NextAuto: next, SystemMotion: motion,
		ReduceMotion: reduce, LowEffects: low, Reason: reason, Persistence: v.persistence}
}

func (v *visualEffects) invalidateSamples() { v.permit = nil; v.badSamples = 0 }

// publishVisualEffects never waits for the native UI thread while holding the state lock.
func (h *HostService) publishVisualEffects(snapshot EffectsSnapshot) {
	application.InvokeAsync(func() {
		// InvokeAsync can execute inline on the UI thread. Never acquire mu here.
		if snapshot.Revision < h.effects.materialRevision {
			return
		}
		h.effects.materialRevision = snapshot.Revision
		low := snapshot.LowEffects
		for _, window := range h.app.Window.GetAll() {
			if w, ok := window.(*application.WebviewWindow); ok {
				applyGUIEffectsMaterial(w, low)
			}
		}
	})
	h.emit(visualEffectsChangedEvent, snapshot)
}

func (h *HostService) startVisualEffects() {
	go func() {
		class := evidenceUnknown
		if runtime.GOOS != "linux" {
			result := make(chan evidenceClass, 1)
			go func() { result <- probeVisualCapabilities() }()
			timer := time.NewTimer(effectsProbeTimeout)
			defer timer.Stop()
			select {
			case class = <-result:
			case <-timer.C:
			}
		}
		h.effects.mu.Lock()
		h.effects.initialize(class)
		snapshot := h.effects.snapshot()
		h.effects.mu.Unlock()
		h.publishVisualEffects(snapshot)
	}()
}

// effectsWindow trusts Wails' call context, never a frontend-provided window label.
func (h *HostService) effectsWindow(ctx context.Context) *application.WebviewWindow {
	w, _ := ctx.Value(application.WindowKey).(*application.WebviewWindow)
	return w
}

func (h *HostService) registerEffectsWindow(w *application.WebviewWindow) {
	if w == nil {
		return
	}
	v := h.effects
	if _, exists := v.windows[w.ID()]; exists {
		return
	}
	// A new document stays conservative locally until it reports; it does not
	// change the preference already established by other windows.
	v.windows[w.ID()] = v.systemMotion()
	w.OnWindowEvent(events.Common.WindowClosing, func(*application.WindowEvent) {
		v.mu.Lock()
		defer v.mu.Unlock()
		delete(v.windows, w.ID())
		v.revision++
		v.invalidateSamples()
		h.publishVisualEffects(v.snapshot())
	})
	for _, event := range []events.WindowEventType{events.Common.WindowHide, events.Common.WindowMinimise} {
		w.OnWindowEvent(event, func(*application.WindowEvent) {
			v.mu.Lock()
			defer v.mu.Unlock()
			if v.permit != nil && v.permit.window == w.ID() {
				v.permit = nil
				v.revision++
				h.publishVisualEffects(v.snapshot())
			}
		})
	}
}

// GetVisualEffects returns this GUI session's authoritative state.
//
//uc:errors none
//uc:os all=real
func (h *HostService) GetVisualEffects() EffectsSnapshot {
	v := h.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	// Wails removes destroyed windows from its manager. Prune their reports
	// before reads as well as environment changes, including missed close events.
	h.pruneEffectsWindows()
	return v.snapshot()
}

func (h *HostService) pruneEffectsWindows() {
	v := h.effects
	changed := false
	for id := range v.windows {
		if _, exists := h.app.Window.GetByID(id); !exists {
			delete(v.windows, id)
			changed = true
		}
	}
	if changed {
		v.revision++
		v.invalidateSamples()
		h.publishVisualEffects(v.snapshot())
	}
}

// SetVisualEffectsMode applies immediately, then saves without rolling back on disk errors.
//
//uc:errors command ValidationError
//uc:os all=real
func (h *HostService) SetVisualEffectsMode(mode EffectsMode) (EffectsSnapshot, error) {
	if mode != EffectsModeAuto && mode != EffectsModeEffects && mode != EffectsModeSmooth {
		return EffectsSnapshot{}, hostapi.New(hostapi.CodeValidationError, "unknown visual effects mode")
	}
	v := h.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	if v.stored.Mode != mode {
		v.invalidateSamples()
	}
	v.stored.Mode = mode
	v.revision++
	v.save()
	snapshot := v.snapshot()
	h.publishVisualEffects(snapshot)
	return snapshot, nil
}

// ReportVisualEffectsEnvironment merges live GUI windows' system preferences.
//
//uc:errors command ValidationError
//uc:os all=real
func (h *HostService) ReportVisualEffectsEnvironment(ctx context.Context, sessionID string, systemMotion SystemMotion) (EffectsSnapshot, error) {
	if systemMotion != SystemMotionReduce && systemMotion != SystemMotionAllow && systemMotion != SystemMotionUnknown {
		return EffectsSnapshot{}, hostapi.New(hostapi.CodeValidationError, "unknown system motion")
	}
	v := h.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	w := h.effectsWindow(ctx)
	if sessionID != v.sessionID || w == nil {
		return v.snapshot(), nil
	}
	before := v.systemMotion()
	h.pruneEffectsWindows()
	h.registerEffectsWindow(w)
	v.windows[w.ID()] = systemMotion
	if before != v.systemMotion() {
		v.revision++
		v.invalidateSamples()
	}
	h.publishVisualEffects(v.snapshot())
	return v.snapshot(), nil
}

// BeginVisualEffectsSample grants one bounded, process-wide foreground measurement.
//
//uc:errors none
//uc:os linux=noop darwin=real windows=real
func (h *HostService) BeginVisualEffectsSample(ctx context.Context, sessionID string) *SamplePermit {
	w := h.effectsWindow(ctx)
	if w == nil || !w.IsVisible() || w.IsMinimised() {
		return nil
	}
	v := h.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	s := v.snapshot()
	now := time.Now()
	if v.platform == "linux" || sessionID != v.sessionID || !v.ready || s.Mode != EffectsModeAuto || s.ReduceMotion || s.LowEffects ||
		v.samples >= effectsMaxSamples || now.Sub(v.started) < effectsWarmup || now.Sub(v.lastSample) < effectsSampleInterval {
		return nil
	}
	v.samples++
	v.lastSample = now
	p := SamplePermit{SessionID: v.sessionID, Revision: v.revision, SampleID: v.samples}
	v.permit = &effectsPermit{SamplePermit: p, window: w.ID(), started: now}
	return &p
}

// ReportVisualEffectsSample validates the permit and saves only a next-launch downgrade.
//
//uc:errors none
//uc:os linux=noop darwin=real windows=real
func (h *HostService) ReportVisualEffectsSample(ctx context.Context, sample EffectsSample) EffectsSnapshot {
	w := h.effectsWindow(ctx)
	visible := w != nil && w.IsVisible() && !w.IsMinimised()
	v := h.effects
	v.mu.Lock()
	defer v.mu.Unlock()
	p := v.permit
	if v.platform == "linux" || !visible || p == nil || w.ID() != p.window ||
		sample.SessionID != v.sessionID || sample.SampleID != p.SampleID || sample.Revision != v.revision ||
		sample.Revision != p.Revision {
		return v.snapshot()
	}
	v.permit = nil
	s := v.snapshot()
	if s.Mode != EffectsModeAuto || s.ReduceMotion || s.LowEffects || time.Since(p.started) > effectsSampleDuration ||
		sample.Frames < effectsMinFrames || sample.Frames > 1000 || sample.LongFrames < 0 || sample.LongFrames > sample.Frames ||
		sample.DurationMs == nil || sample.LongestMs == nil {
		return s
	}
	duration, longest := *sample.DurationMs, *sample.LongestMs
	if math.IsNaN(duration) || math.IsInf(duration, 0) || math.IsNaN(longest) || math.IsInf(longest, 0) ||
		duration <= 0 || duration > float64(time.Since(p.started).Milliseconds())+1 || duration > float64(effectsSampleDuration.Milliseconds()) || longest <= 0 || longest > effectsMaxStallMs || longest > duration || (sample.LongFrames > 0) != (longest > effectsLongFrameMs) ||
		float64(sample.LongFrames)*effectsLongFrameMs >= duration || longest*float64(sample.Frames) < duration {
		return s
	}
	if sample.LongFrames >= effectsMinLongFrames && float64(sample.LongFrames)/float64(sample.Frames) >= effectsBadFraction {
		v.badSamples++
		if v.badSamples >= effectsBadSamples && (v.stored.NextAuto == nil || *v.stored.NextAuto != AutoResultSmooth || v.persistence == EffectsPersistenceSessionOnly) {
			next := AutoResultSmooth
			v.stored.NextAuto = &next
			v.revision++
			v.save()
			h.publishVisualEffects(v.snapshot())
		}
	}
	return v.snapshot()
}
