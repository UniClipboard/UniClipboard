package mobile

import (
	"encoding/json"
	"sync"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/observability"
	"github.com/uniclipboard/unic/internal/procmetrics"
)

const mobileResourceInterval = 60 * time.Second

// HostResourceProvider supplies OS-level CPU, memory, thermal, battery, and
// network snapshots from the native iOS or Android host.
type HostResourceProvider interface {
	CollectResourceSnapshot(epoch int64) string
}

var (
	globalHostResourceMu       sync.RWMutex
	globalHostResourceProvider HostResourceProvider
)

// RegisterHostResourceProvider installs the process-wide native resource provider.
func RegisterHostResourceProvider(provider HostResourceProvider) {
	globalHostResourceMu.Lock()
	globalHostResourceProvider = provider
	globalHostResourceMu.Unlock()
}

type hostResourceEnvelope struct {
	Resource     *observability.ResourceSample `json:"resource,omitempty"`
	Power        *observability.PowerSample    `json:"power,omitempty"`
	PowerSamples []observability.PowerSample   `json:"power_samples,omitempty"`
}

// SetHostResourceProvider sets a per-Engine native resource provider override
// and primes it against the active metrics generation epoch if enabled.
func (m *Engine) SetHostResourceProvider(provider HostResourceProvider) {
	m.resourceMu.Lock()
	m.resourceProvider = provider
	m.resourceMu.Unlock()
	if provider != nil && !m.diagnostics.isClosed() {
		if epoch := m.diagnostics.controller.SyncEpoch(); epoch > 0 {
			func() {
				defer func() { _ = recover() }()
				_ = provider.CollectResourceSnapshot(int64(epoch))
			}()
		}
	}
}

func (m *Engine) currentHostResourceProvider() HostResourceProvider {
	m.resourceMu.RLock()
	p := m.resourceProvider
	m.resourceMu.RUnlock()
	if p != nil {
		return p
	}
	globalHostResourceMu.RLock()
	p = globalHostResourceProvider
	globalHostResourceMu.RUnlock()
	return p
}

func (m *Engine) defaultProcessRole() string {
	if m.extensionMode {
		return "ios_keyboard"
	}
	return "mobile_app"
}

func (m *Engine) normalizePowerSample(p *observability.PowerSample, role string) {
	if m.extensionMode || p.ProcessRole == "" {
		p.ProcessRole = role
	}
	if p.AppState == "" {
		p.AppState = "foreground"
	}
	if p.PowerSource == "" {
		p.PowerSource = "unknown"
	}
}

func (m *Engine) normalizeEnvelope(env *hostResourceEnvelope) {
	role := m.defaultProcessRole()
	heap := procmetrics.GoHeapBytes()
	if env.Resource != nil {
		if m.extensionMode || env.Resource.ProcessRole == "" {
			env.Resource.ProcessRole = role
		}
		if env.Resource.AppState == "" {
			env.Resource.AppState = "foreground"
		}
		if env.Resource.PowerSource == "" {
			env.Resource.PowerSource = "unknown"
		}
		env.Resource.GoHeapBytes = heap
	}
	if env.Power != nil {
		m.normalizePowerSample(env.Power, role)
	}
	for i := range env.PowerSamples {
		m.normalizePowerSample(&env.PowerSamples[i], role)
	}
}

func (m *Engine) collectResources(epoch uint64) ([]observability.ResourceSample, []observability.PowerSample) {
	if p := m.currentHostResourceProvider(); p != nil {
		var raw string
		func() {
			defer func() { _ = recover() }()
			raw = p.CollectResourceSnapshot(int64(epoch))
		}()
		if epoch == 0 || raw == "" || raw == "{}" {
			return nil, nil
		}
		var env hostResourceEnvelope
		if err := json.Unmarshal([]byte(raw), &env); err != nil {
			// Return an invalid sample so the controller counts invalid_record.
			return []observability.ResourceSample{{ProcessRole: "invalid"}}, nil
		}
		m.normalizeEnvelope(&env)
		var res []observability.ResourceSample
		var pwr []observability.PowerSample
		if env.Resource != nil {
			res = append(res, *env.Resource)
		}
		if env.Power != nil {
			pwr = append(pwr, *env.Power)
		}
		if len(env.PowerSamples) > 0 {
			pwr = append(pwr, env.PowerSamples...)
		}
		return res, pwr
	}

	m.fallbackSamplerMu.Lock()
	defer m.fallbackSamplerMu.Unlock()
	if epoch == 0 {
		m.fallbackSampler.Reset()
		m.fallbackEpoch = 0
		return nil, nil
	}
	if epoch != m.fallbackEpoch {
		m.fallbackSampler.Reset()
		m.fallbackSampler.Prime()
		m.fallbackEpoch = epoch
		return nil, nil
	}
	d := m.fallbackSampler.Sample(mobileResourceInterval)
	if !d.HasWindow {
		return nil, nil
	}
	return []observability.ResourceSample{{
		ProcessRole:           m.defaultProcessRole(),
		AppState:              "foreground",
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

// SubmitPowerWindow admits an immediate resource/power transition sample into
// the active metrics generation (for example on foreground/background switch).
func (m *Engine) SubmitPowerWindow(windowJSON string) error {
	if m.diagnostics.isClosed() {
		return wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	var env hostResourceEnvelope
	if err := json.Unmarshal([]byte(windowJSON), &env); err != nil {
		return wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "invalid power window json"})
	}
	m.normalizeEnvelope(&env)
	if env.Resource != nil {
		m.diagnostics.controller.RecordResourceSample(*env.Resource)
	}
	if env.Power != nil {
		m.diagnostics.controller.RecordPowerSample(*env.Power)
	}
	for i := range env.PowerSamples {
		m.diagnostics.controller.RecordPowerSample(env.PowerSamples[i])
	}
	return nil
}

// SubmitMetricKitPayload admits a sanitized 24-hour iOS MetricKit summary.
func (m *Engine) SubmitMetricKitPayload(payloadJSON string) error {
	if m.diagnostics.isClosed() {
		return wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	if m.extensionMode {
		return wrapErr(&engine.Error{Code: engine.CodeUnsupported, Message: "metrickit is main-app only"})
	}
	var report observability.MetricKitReport
	if err := json.Unmarshal([]byte(payloadJSON), &report); err != nil {
		return wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "invalid metrickit payload json"})
	}
	if err := m.diagnostics.controller.RecordMetricKit(report); err != nil {
		switch err.Error() {
		case "invalid_argument":
			return wrapErr(&engine.Error{Code: engine.CodeInvalidArgument, Message: "invalid metrickit values"})
		case "resource_exhausted":
			return wrapErr(&engine.Error{Code: engine.CodeInternal, Message: "metrickit queue full"})
		default:
			return wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "metrickit telemetry unavailable"})
		}
	}
	return nil
}

// FlushResourceSample triggers one synchronous resource/power collection into
// the active metrics generation before background shutdown or extension teardown.
func (m *Engine) FlushResourceSample() error {
	if m.diagnostics.isClosed() {
		return wrapErr(&engine.Error{Code: engine.CodeInvalidState, Message: "mobile diagnostics closed"})
	}
	epoch := m.diagnostics.controller.SyncEpoch()
	if epoch == 0 {
		return nil
	}
	res, pwr := m.collectResources(epoch)
	for i := range res {
		m.diagnostics.controller.RecordResourceSample(res[i])
	}
	for i := range pwr {
		m.diagnostics.controller.RecordPowerSample(pwr[i])
	}
	return nil
}
