package procmetrics

import (
	"bytes"
	"strconv"
	"strings"
	"sync"
	"time"
)

// ProbeStatus reports whether an OS probe returned a valid value, is unsupported
// on the current OS/kernel, or failed transiently.
type ProbeStatus string

const (
	StatusOK          ProbeStatus = "ok"
	StatusUnsupported ProbeStatus = "unsupported"
	StatusError       ProbeStatus = "error"
)

// Snapshot holds raw cumulative or instantaneous process resource counters.
type Snapshot struct {
	Monotonic             time.Time
	BootElapsed           time.Duration
	CPUTime               float64
	CPUStatus             ProbeStatus
	MemoryFootprintBytes  uint64
	MemoryStatus          ProbeStatus
	GoHeapBytes           uint64
	Wakeups               uint64
	WakeupsStatus         ProbeStatus
	ContextSwitches       uint64
	ContextSwitchesStatus ProbeStatus
}

// Delta holds interval-delimited resource increments between consecutive samples.
type Delta struct {
	HasWindow             bool
	StartAt               time.Time
	EndAt                 time.Time
	WindowSeconds         float64
	SleepJumped           bool
	CPUSeconds            float64
	CPUStatus             ProbeStatus
	MemoryFootprintBytes  uint64
	MemoryStatus          ProbeStatus
	GoHeapBytes           uint64
	Wakeups               uint64
	WakeupsStatus         ProbeStatus
	ContextSwitches       uint64
	ContextSwitchesStatus ProbeStatus
}

// Sampler computes monotonic resource deltas while guarding against sleep jumps,
// counter regressions, and disabled-period replay.
type Sampler struct {
	mu      sync.Mutex
	hasPrev bool
	prev    Snapshot
	probeFn func() Snapshot
}

// NewSampler constructs a Sampler backed by the current platform OS probes.
func NewSampler() *Sampler {
	return &Sampler{probeFn: readPlatformSnapshot}
}

// NewSamplerWithProbe constructs a Sampler with an injected probe function for testing.
func NewSamplerWithProbe(fn func() Snapshot) *Sampler {
	if fn == nil {
		fn = readPlatformSnapshot
	}
	return &Sampler{probeFn: fn}
}

// Reset discards the stored baseline so disabled-period activity is never replayed.
func (s *Sampler) Reset() {
	s.mu.Lock()
	s.hasPrev = false
	s.prev = Snapshot{}
	s.mu.Unlock()
}

// Prime captures a fresh baseline snapshot without emitting a window delta.
func (s *Sampler) Prime() {
	snap := s.probeFn()
	s.mu.Lock()
	s.prev = snap
	s.hasPrev = true
	s.mu.Unlock()
}

// Sample reads the current snapshot and returns the delta since the previous Sample/Prime.
// If expectedInterval > 0 and boot time jumped beyond max(3*expectedInterval, mono+10s),
// SleepJumped is set so callers skip utilization attribution across deep sleep.
func (s *Sampler) Sample(expectedInterval time.Duration) Delta {
	snap := s.probeFn()
	s.mu.Lock()
	defer s.mu.Unlock()

	out := Delta{
		EndAt:                 snap.Monotonic,
		MemoryFootprintBytes:  snap.MemoryFootprintBytes,
		MemoryStatus:          snap.MemoryStatus,
		GoHeapBytes:           snap.GoHeapBytes,
		CPUStatus:             snap.CPUStatus,
		WakeupsStatus:         snap.WakeupsStatus,
		ContextSwitchesStatus: snap.ContextSwitchesStatus,
	}
	if !s.hasPrev {
		s.prev = snap
		s.hasPrev = true
		return out
	}

	out.HasWindow = true
	out.StartAt = s.prev.Monotonic
	monoDelta := snap.Monotonic.Sub(s.prev.Monotonic)
	bootDelta := snap.BootElapsed - s.prev.BootElapsed

	if monoDelta <= 0 {
		out.SleepJumped = true
	} else {
		out.WindowSeconds = monoDelta.Seconds()
	}

	if snap.BootElapsed > 0 && s.prev.BootElapsed > 0 {
		if bootDelta < 0 || bootDelta-monoDelta > 10*time.Second {
			out.SleepJumped = true
		} else if expectedInterval > 0 && bootDelta > 3*expectedInterval {
			out.SleepJumped = true
		}
	} else if expectedInterval > 0 && monoDelta > 3*expectedInterval {
		out.SleepJumped = true
	}

	if snap.CPUStatus == StatusOK && s.prev.CPUStatus == StatusOK {
		if snap.CPUTime < s.prev.CPUTime {
			out.CPUStatus = StatusError
		} else {
			out.CPUSeconds = snap.CPUTime - s.prev.CPUTime
			out.CPUStatus = StatusOK
		}
	} else if snap.CPUStatus == StatusOK {
		out.CPUStatus = s.prev.CPUStatus
	}

	if snap.WakeupsStatus == StatusOK && s.prev.WakeupsStatus == StatusOK {
		if snap.Wakeups < s.prev.Wakeups {
			out.WakeupsStatus = StatusError
		} else {
			out.Wakeups = snap.Wakeups - s.prev.Wakeups
			out.WakeupsStatus = StatusOK
		}
	} else if snap.WakeupsStatus == StatusOK {
		out.WakeupsStatus = s.prev.WakeupsStatus
	}

	if snap.ContextSwitchesStatus == StatusOK && s.prev.ContextSwitchesStatus == StatusOK {
		if snap.ContextSwitches < s.prev.ContextSwitches {
			out.ContextSwitchesStatus = StatusError
		} else {
			out.ContextSwitches = snap.ContextSwitches - s.prev.ContextSwitches
			out.ContextSwitchesStatus = StatusOK
		}
	} else if snap.ContextSwitchesStatus == StatusOK {
		out.ContextSwitchesStatus = s.prev.ContextSwitchesStatus
	}

	s.prev = snap
	return out
}

// ParseProcField parses an integer value from a Linux/Android /proc/self/* line
// formatted as "Key:   1234 kB" or "Key:\t1234". When multiplier > 1, the parsed
// value is scaled (e.g. 1024 for KiB fields).
func ParseProcField(data []byte, key string, multiplier uint64) (uint64, bool) {
	if multiplier == 0 {
		multiplier = 1
	}
	prefix := key + ":"
	for _, rawLine := range bytes.Split(data, []byte{'\n'}) {
		line := strings.TrimSpace(string(rawLine))
		if !strings.HasPrefix(line, prefix) {
			continue
		}
		rest := strings.TrimSpace(strings.TrimPrefix(line, prefix))
		fields := strings.Fields(rest)
		if len(fields) == 0 {
			return 0, false
		}
		val, err := strconv.ParseUint(fields[0], 10, 64)
		if err != nil {
			return 0, false
		}
		return val * multiplier, true
	}
	return 0, false
}
