package host

import (
	"sync"
	"time"

	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/procmetrics"
)

func newServiceResourceCollector(expectedInterval time.Duration) func(uint64) ([]observability.ResourceSample, []observability.PowerSample) {
	if expectedInterval <= 0 {
		expectedInterval = 60 * time.Second
	}
	sampler := procmetrics.NewSampler()
	var mu sync.Mutex
	var lastEpoch uint64
	return func(epoch uint64) ([]observability.ResourceSample, []observability.PowerSample) {
		mu.Lock()
		defer mu.Unlock()
		if epoch == 0 {
			sampler.Reset()
			lastEpoch = 0
			return nil, nil
		}
		if epoch != lastEpoch {
			sampler.Reset()
			sampler.Prime()
			lastEpoch = epoch
			return nil, nil
		}
		d := sampler.Sample(expectedInterval)
		if !d.HasWindow {
			return nil, nil
		}
		return []observability.ResourceSample{{
			ProcessRole:           "service",
			AppState:              "background",
			PowerSource:           "unknown",
			WindowSeconds:         d.WindowSeconds,
			CPUSeconds:            d.CPUSeconds,
			CPUProbeStatus:        string(d.CPUStatus),
			SleepJumped:           d.SleepJumped,
			MemoryFootprintBytes:  d.MemoryFootprintBytes,
			MemoryProbeStatus:     string(d.MemoryStatus),
			GoHeapBytes:           d.GoHeapBytes,
			Wakeups:               d.Wakeups,
			WakeupsProbeStatus:    string(d.WakeupsStatus),
			ContextSwitches:       d.ContextSwitches,
			ContextSwitchesStatus: string(d.ContextSwitchesStatus),
		}}, nil
	}
}
