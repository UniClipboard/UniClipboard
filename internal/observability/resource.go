package observability

import (
	"context"
	"errors"
	"math"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/metric"
)

const (
	minCPUUtilWindowSeconds         = 10.0
	minKeyboardCPUUtilWindowSeconds = 5.0
)

// ResourceSample holds a bounded, anonymous process resource measurement for
// one observation window.
type ResourceSample struct {
	ProcessRole           string  `json:"process_role"`
	AppState              string  `json:"app_state"`
	PowerSource           string  `json:"power_source,omitempty"`
	WindowSeconds         float64 `json:"window_seconds"`
	CPUSeconds            float64 `json:"cpu_seconds"`
	CPUProbeStatus        string  `json:"cpu_probe_status,omitempty"`
	SleepJumped           bool    `json:"sleep_jumped,omitempty"`
	MemoryFootprintBytes  uint64  `json:"memory_footprint_bytes,omitempty"`
	MemoryProbeStatus     string  `json:"memory_probe_status,omitempty"`
	GoHeapBytes           uint64  `json:"go_heap_bytes,omitempty"`
	Wakeups               uint64  `json:"wakeups,omitempty"`
	WakeupsProbeStatus    string  `json:"wakeups_probe_status,omitempty"`
	ContextSwitches       uint64  `json:"context_switches,omitempty"`
	ContextSwitchesStatus string  `json:"context_switches_probe_status,omitempty"`
}

// PowerSample holds mobile power-proxy and system-level readings.
type PowerSample struct {
	ProcessRole        string   `json:"process_role"`
	AppState           string   `json:"app_state"`
	PowerSource        string   `json:"power_source"`
	TxBytes            *int64   `json:"tx_bytes,omitempty"`
	RxBytes            *int64   `json:"rx_bytes,omitempty"`
	NetworkProbeStatus string   `json:"network_probe_status,omitempty"`
	ThermalLevel       string   `json:"thermal_level,omitempty"`
	ThermalSeconds     float64  `json:"thermal_seconds,omitempty"`
	ThermalProbeStatus string   `json:"thermal_probe_status,omitempty"`
	BgMode             string   `json:"bg_mode,omitempty"`
	ScreenState        string   `json:"screen_state,omitempty"`
	MulticastLock      *bool    `json:"multicast_lock,omitempty"`
	BgModeSeconds      float64  `json:"bg_mode_seconds,omitempty"`
	BatteryDrainRate   *float64 `json:"battery_drain_rate,omitempty"`
	LowPowerMode       *bool    `json:"low_power_mode,omitempty"`
	BatteryProbeStatus string   `json:"battery_probe_status,omitempty"`
}

// MetricKitReport holds sanitized 24-hour iOS MetricKit summaries from the main app.
type MetricKitReport struct {
	CPUSeconds        *float64 `json:"cpu_seconds,omitempty"`
	WifiTxBytes       *int64   `json:"wifi_tx_bytes,omitempty"`
	WifiRxBytes       *int64   `json:"wifi_rx_bytes,omitempty"`
	CellularTxBytes   *int64   `json:"cellular_tx_bytes,omitempty"`
	CellularRxBytes   *int64   `json:"cellular_rx_bytes,omitempty"`
	PeakMemoryBytes   *uint64  `json:"peak_memory_bytes,omitempty"`
	ForegroundSeconds *float64 `json:"foreground_seconds,omitempty"`
	BackgroundSeconds *float64 `json:"background_seconds,omitempty"`
}

func validProcessRole(v string) bool {
	switch v {
	case "service", "tray", "gui", "quick_panel", "mobile_app", "ios_keyboard":
		return true
	}
	return false
}

func validDesktopProcessRole(v string) bool {
	switch v {
	case "tray", "gui", "quick_panel":
		return true
	}
	return false
}

func validAppState(v string) bool {
	switch v {
	case "foreground", "background", "background_screen_off", "suspended":
		return true
	}
	return false
}

func validPowerSource(v string) bool {
	switch v {
	case "battery", "plugged", "unknown":
		return true
	}
	return false
}

func validProbeStatus(v string) bool {
	switch v {
	case "", "ok", "unsupported", "error":
		return true
	}
	return false
}

func validThermalLevel(v string) bool {
	switch v {
	case "nominal", "fair", "serious", "critical":
		return true
	}
	return false
}

func validBgMode(v string) bool {
	switch v {
	case "fgs_idle", "overlay_polling", "overlay_event", "shizuku":
		return true
	}
	return false
}

func validScreenState(v string) bool {
	return v == "on" || v == "off"
}

func validNonNegativeFloat(v float64) bool {
	return !math.IsNaN(v) && !math.IsInf(v, 0) && v >= 0
}

// ValidateResourceSample checks finite enums and non-negative numeric bounds.
func ValidateResourceSample(r *ResourceSample) bool {
	if r == nil {
		return false
	}
	if r.PowerSource == "" {
		r.PowerSource = "unknown"
	}
	if !validProcessRole(r.ProcessRole) || !validAppState(r.AppState) || !validPowerSource(r.PowerSource) {
		return false
	}
	if !validNonNegativeFloat(r.WindowSeconds) || !validNonNegativeFloat(r.CPUSeconds) {
		return false
	}
	if !validProbeStatus(r.CPUProbeStatus) || !validProbeStatus(r.MemoryProbeStatus) ||
		!validProbeStatus(r.WakeupsProbeStatus) || !validProbeStatus(r.ContextSwitchesStatus) {
		return false
	}
	return true
}

// ValidatePowerSample checks finite enums and non-negative numeric bounds.
func ValidatePowerSample(p *PowerSample) bool {
	if p == nil {
		return false
	}
	if p.ProcessRole != "mobile_app" && p.ProcessRole != "ios_keyboard" {
		return false
	}
	if !validAppState(p.AppState) || !validPowerSource(p.PowerSource) {
		return false
	}
	if !validProbeStatus(p.NetworkProbeStatus) || !validProbeStatus(p.ThermalProbeStatus) || !validProbeStatus(p.BatteryProbeStatus) {
		return false
	}
	if p.TxBytes != nil && *p.TxBytes < 0 {
		return false
	}
	if p.RxBytes != nil && *p.RxBytes < 0 {
		return false
	}
	if !validNonNegativeFloat(p.ThermalSeconds) || !validNonNegativeFloat(p.BgModeSeconds) {
		return false
	}
	if p.ThermalLevel != "" && !validThermalLevel(p.ThermalLevel) {
		return false
	}
	if p.BgMode != "" && (!validBgMode(p.BgMode) || !validScreenState(p.ScreenState) || p.MulticastLock == nil) {
		return false
	}
	if p.BatteryDrainRate != nil && (!validNonNegativeFloat(*p.BatteryDrainRate) || p.LowPowerMode == nil) {
		return false
	}
	return true
}

// ValidateMetricKitReport checks non-negative finite MetricKit values.
func ValidateMetricKitReport(r *MetricKitReport) bool {
	if r == nil {
		return false
	}
	for _, f := range []*float64{r.CPUSeconds, r.ForegroundSeconds, r.BackgroundSeconds} {
		if f != nil && !validNonNegativeFloat(*f) {
			return false
		}
	}
	for _, b := range []*int64{r.WifiTxBytes, r.WifiRxBytes, r.CellularTxBytes, r.CellularRxBytes} {
		if b != nil && *b < 0 {
			return false
		}
	}
	return true
}

// SetResourceCollector installs the host resource collector called on metrics
// collection and reset on generation teardown/disable.
func (c *Controller) SetResourceCollector(f func(epoch uint64) ([]ResourceSample, []PowerSample)) {
	c.mu.Lock()
	c.resourceCollector = f
	c.resCollectorSeq++
	g := c.metrics
	c.mu.Unlock()
	if f != nil && g != nil {
		g.primeResources()
	}
}

// RecordDesktopResourceSample validates and admits a resource sample submitted
// over the local socket by a Desktop role process.
func (c *Controller) RecordDesktopResourceSample(r ResourceSample) error {
	if !validDesktopProcessRole(r.ProcessRole) || r.WindowSeconds <= 0 || !ValidateResourceSample(&r) {
		c.mu.Lock()
		closed := c.closed
		c.mu.Unlock()
		if closed {
			return errors.New("unavailable")
		}
		c.drop(2, 2, 1)
		return errors.New("invalid_argument")
	}
	c.mu.Lock()
	closed := c.closed
	g := c.metrics
	c.mu.Unlock()
	if closed {
		return errors.New("unavailable")
	}
	if g == nil {
		return nil
	}
	// Bounded drop on queue_full is recorded by enqueueResource; disabled/inactive
	// drops remain silent per local-service-contract (HTTP 204).
	_ = g.enqueueResource(metricRecord{resource: &r})
	return nil
}

// RecordResourceSample admits a validated host resource sample into the active metrics generation.
func (c *Controller) RecordResourceSample(r ResourceSample) {
	c.mu.Lock()
	g := c.metrics
	c.mu.Unlock()
	if g == nil {
		return
	}
	if !ValidateResourceSample(&r) {
		c.drop(2, 2, 1)
		return
	}
	// Bounded drop on queue_full is recorded inside enqueueResource.
	_ = g.enqueueResource(metricRecord{resource: &r})
}

// RecordPowerSample admits a validated mobile power sample into the active metrics generation.
func (c *Controller) RecordPowerSample(p PowerSample) {
	c.mu.Lock()
	g := c.metrics
	c.mu.Unlock()
	if g == nil {
		return
	}
	if !ValidatePowerSample(&p) {
		c.drop(2, 2, 1)
		return
	}
	// Bounded drop on queue_full is recorded inside enqueueResource.
	_ = g.enqueueResource(metricRecord{power: &p})
}

// RecordMetricKit admits a validated 24-hour iOS MetricKit summary into the active metrics generation.
func (c *Controller) RecordMetricKit(r MetricKitReport) error {
	c.mu.Lock()
	g := c.metrics
	closed := c.closed
	c.mu.Unlock()
	if closed || g == nil {
		return errors.New("unavailable")
	}
	if !ValidateMetricKitReport(&r) {
		c.drop(2, 2, 1)
		return errors.New("invalid_argument")
	}
	return g.enqueueResource(metricRecord{metrickit: &r})
}

func (g *generation) enqueueResource(r metricRecord) error {
	g.mu.Lock()
	defer g.mu.Unlock()
	if !g.active {
		return errors.New("unavailable")
	}
	select {
	case g.diagnostics <- r:
		return nil
	default:
		g.c.drop(2, 0, 1)
		return errors.New("resource_exhausted")
	}
}

func safeResourceCollect(f func(uint64) ([]ResourceSample, []PowerSample), epoch uint64) (res []ResourceSample, pwr []PowerSample) {
	defer func() { _ = recover() }()
	return f(epoch)
}

func (g *generation) primeResources() {
	g.resMu.Lock()
	defer g.resMu.Unlock()
	g.c.mu.Lock()
	f := g.c.resourceCollector
	seq := g.c.resCollectorSeq
	g.c.mu.Unlock()
	if f == nil || g.resPrimedSeq == seq {
		return
	}
	res, pwr := safeResourceCollect(f, g.epoch)
	g.resPrimedSeq = seq
	g.recordCollectedResources(res, pwr)
}

func (g *generation) collectResources() {
	g.resMu.Lock()
	defer g.resMu.Unlock()
	g.c.mu.Lock()
	f := g.c.resourceCollector
	seq := g.c.resCollectorSeq
	g.c.mu.Unlock()
	if f == nil {
		return
	}
	res, pwr := safeResourceCollect(f, g.epoch)
	g.resPrimedSeq = seq
	g.recordCollectedResources(res, pwr)
}

func (g *generation) recordCollectedResources(res []ResourceSample, pwr []PowerSample) {
	for i := range res {
		if ValidateResourceSample(&res[i]) {
			g.recordResource(res[i])
		} else {
			g.c.drop(2, 2, 1)
		}
	}
	for i := range pwr {
		if ValidatePowerSample(&pwr[i]) {
			g.recordPower(pwr[i])
		} else {
			g.c.drop(2, 2, 1)
		}
	}
}

func (g *generation) initResourceMetrics() error {
	m := g.meter.Meter("unic.observability", metric.WithInstrumentationVersion("1"))
	var err error
	if g.inst.resCPU, err = m.Float64Counter("unic.resource.cpu.time", metric.WithUnit("s")); err != nil {
		return err
	}
	if g.inst.resWindow, err = m.Float64Counter("unic.resource.window.time", metric.WithUnit("s")); err != nil {
		return err
	}
	if g.inst.resCPUUtil, err = m.Float64Histogram("unic.resource.cpu.utilization", metric.WithUnit("1"),
		metric.WithExplicitBucketBoundaries(0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1, 2, 4)); err != nil {
		return err
	}
	memBounds := []float64{
		4 << 20, 8 << 20, 16 << 20, 24 << 20, 32 << 20, 48 << 20, 64 << 20,
		96 << 20, 128 << 20, 192 << 20, 256 << 20, 384 << 20, 512 << 20, 1024 << 20,
	}
	if g.inst.resMemory, err = m.Float64Histogram("unic.resource.memory.footprint", metric.WithUnit("By"),
		metric.WithExplicitBucketBoundaries(memBounds...)); err != nil {
		return err
	}
	goHeapBounds := []float64{
		1 << 20, 2 << 20, 4 << 20, 8 << 20, 16 << 20, 32 << 20, 64 << 20, 128 << 20, 256 << 20, 512 << 20,
	}
	if g.inst.resGoHeap, err = m.Float64Histogram("unic.resource.memory.go_heap", metric.WithUnit("By"),
		metric.WithExplicitBucketBoundaries(goHeapBounds...)); err != nil {
		return err
	}
	if g.inst.resProbeStatus, err = m.Int64Counter("unic.resource.probe_status", metric.WithUnit("{sample}")); err != nil {
		return err
	}
	if g.inst.pwrNetwork, err = m.Int64Counter("unic.power.network.bytes", metric.WithUnit("By")); err != nil {
		return err
	}
	if g.inst.pwrWakeups, err = m.Int64Counter("unic.power.wakeups", metric.WithUnit("{wakeup}")); err != nil {
		return err
	}
	if g.inst.pwrContextSwitches, err = m.Int64Counter("unic.power.context_switches", metric.WithUnit("{switch}")); err != nil {
		return err
	}
	if g.inst.pwrBgMode, err = m.Float64Counter("unic.power.bg_mode.time", metric.WithUnit("s")); err != nil {
		return err
	}
	if g.inst.pwrThermal, err = m.Float64Counter("unic.power.thermal.time", metric.WithUnit("s")); err != nil {
		return err
	}
	if g.inst.pwrDrainRate, err = m.Float64Histogram("unic.power.system_battery.drain_rate", metric.WithUnit("{percent}/h"),
		metric.WithExplicitBucketBoundaries(0, 0.5, 1, 2, 3, 5, 8, 12, 20, 35, 60)); err != nil {
		return err
	}
	if g.inst.mkCPU, err = m.Float64Counter("unic.power.ios_metrickit.cpu.time", metric.WithUnit("s")); err != nil {
		return err
	}
	if g.inst.mkNetwork, err = m.Int64Counter("unic.power.ios_metrickit.network.bytes", metric.WithUnit("By")); err != nil {
		return err
	}
	if g.inst.mkMemoryPeak, err = m.Float64Histogram("unic.power.ios_metrickit.memory.peak", metric.WithUnit("By"),
		metric.WithExplicitBucketBoundaries(memBounds...)); err != nil {
		return err
	}
	if g.inst.mkAppTime, err = m.Float64Counter("unic.power.ios_metrickit.app_time", metric.WithUnit("s")); err != nil {
		return err
	}
	return nil
}

func (g *generation) recordProbeStatus(role, probe, status string) {
	if status == "unsupported" || status == "error" {
		g.inst.resProbeStatus.Add(context.Background(), 1, metric.WithAttributes(
			attribute.String("process_role", role),
			attribute.String("probe", probe),
			attribute.String("status", status),
		))
	}
}

func (g *generation) recordResource(r ResourceSample) {
	ctx := context.Background()
	roleAttr := attribute.String("process_role", r.ProcessRole)
	stateAttr := attribute.String("app_state", r.AppState)
	powerAttr := attribute.String("power_source", r.PowerSource)

	if r.CPUProbeStatus == "unsupported" || r.CPUProbeStatus == "error" {
		g.recordProbeStatus(r.ProcessRole, "cpu", r.CPUProbeStatus)
	} else if !r.SleepJumped && r.WindowSeconds > 0 {
		g.inst.resWindow.Add(ctx, r.WindowSeconds, metric.WithAttributes(roleAttr, stateAttr, powerAttr))
		g.inst.resCPU.Add(ctx, r.CPUSeconds, metric.WithAttributes(roleAttr, stateAttr, powerAttr))
		minWin := minCPUUtilWindowSeconds
		if r.ProcessRole == "ios_keyboard" {
			minWin = minKeyboardCPUUtilWindowSeconds
		}
		if r.WindowSeconds >= minWin {
			g.inst.resCPUUtil.Record(ctx, r.CPUSeconds/r.WindowSeconds, metric.WithAttributes(roleAttr, stateAttr, powerAttr))
		}
	}

	if r.MemoryProbeStatus == "unsupported" || r.MemoryProbeStatus == "error" {
		g.recordProbeStatus(r.ProcessRole, "memory", r.MemoryProbeStatus)
	} else if r.MemoryFootprintBytes > 0 {
		g.inst.resMemory.Record(ctx, float64(r.MemoryFootprintBytes), metric.WithAttributes(roleAttr, stateAttr))
	}

	if r.GoHeapBytes > 0 {
		g.inst.resGoHeap.Record(ctx, float64(r.GoHeapBytes), metric.WithAttributes(roleAttr, stateAttr))
	}

	if r.WakeupsProbeStatus == "unsupported" || r.WakeupsProbeStatus == "error" {
		g.recordProbeStatus(r.ProcessRole, "wakeups", r.WakeupsProbeStatus)
	} else if r.WakeupsProbeStatus == "ok" && !r.SleepJumped {
		g.inst.pwrWakeups.Add(ctx, int64(r.Wakeups), metric.WithAttributes(roleAttr, stateAttr))
	}

	if r.ContextSwitchesStatus == "unsupported" || r.ContextSwitchesStatus == "error" {
		g.recordProbeStatus(r.ProcessRole, "context_switches", r.ContextSwitchesStatus)
	} else if r.ContextSwitchesStatus == "ok" && !r.SleepJumped {
		g.inst.pwrContextSwitches.Add(ctx, int64(r.ContextSwitches), metric.WithAttributes(roleAttr, stateAttr))
	}
}

func (g *generation) recordPower(p PowerSample) {
	ctx := context.Background()
	stateAttr := attribute.String("app_state", p.AppState)
	powerAttr := attribute.String("power_source", p.PowerSource)

	if p.NetworkProbeStatus == "unsupported" || p.NetworkProbeStatus == "error" {
		g.recordProbeStatus(p.ProcessRole, "network", p.NetworkProbeStatus)
	} else {
		if p.TxBytes != nil {
			g.inst.pwrNetwork.Add(ctx, *p.TxBytes, metric.WithAttributes(attribute.String("direction", "tx"), stateAttr))
		}
		if p.RxBytes != nil {
			g.inst.pwrNetwork.Add(ctx, *p.RxBytes, metric.WithAttributes(attribute.String("direction", "rx"), stateAttr))
		}
	}

	if p.ThermalProbeStatus == "unsupported" || p.ThermalProbeStatus == "error" {
		g.recordProbeStatus(p.ProcessRole, "thermal", p.ThermalProbeStatus)
	} else if p.ThermalLevel != "" && p.ThermalSeconds > 0 {
		g.inst.pwrThermal.Add(ctx, p.ThermalSeconds, metric.WithAttributes(stateAttr, attribute.String("level", p.ThermalLevel), powerAttr))
	}

	if p.BgMode != "" && p.BgModeSeconds > 0 && p.MulticastLock != nil {
		mcast := "false"
		if *p.MulticastLock {
			mcast = "true"
		}
		g.inst.pwrBgMode.Add(ctx, p.BgModeSeconds, metric.WithAttributes(
			attribute.String("mode", p.BgMode),
			attribute.String("screen_state", p.ScreenState),
			attribute.String("multicast_lock", mcast),
		))
	}

	if p.BatteryProbeStatus == "unsupported" || p.BatteryProbeStatus == "error" {
		g.recordProbeStatus(p.ProcessRole, "battery", p.BatteryProbeStatus)
	} else if p.BatteryDrainRate != nil && p.LowPowerMode != nil {
		lpm := "false"
		if *p.LowPowerMode {
			lpm = "true"
		}
		g.inst.pwrDrainRate.Record(ctx, *p.BatteryDrainRate, metric.WithAttributes(stateAttr, attribute.String("low_power_mode", lpm)))
	}
}

func (g *generation) recordMetricKit(r MetricKitReport) {
	ctx := context.Background()
	if r.CPUSeconds != nil {
		g.inst.mkCPU.Add(ctx, *r.CPUSeconds)
	}
	if r.WifiTxBytes != nil {
		g.inst.mkNetwork.Add(ctx, *r.WifiTxBytes, metric.WithAttributes(attribute.String("direction", "tx"), attribute.String("transport", "wifi")))
	}
	if r.WifiRxBytes != nil {
		g.inst.mkNetwork.Add(ctx, *r.WifiRxBytes, metric.WithAttributes(attribute.String("direction", "rx"), attribute.String("transport", "wifi")))
	}
	if r.CellularTxBytes != nil {
		g.inst.mkNetwork.Add(ctx, *r.CellularTxBytes, metric.WithAttributes(attribute.String("direction", "tx"), attribute.String("transport", "cellular")))
	}
	if r.CellularRxBytes != nil {
		g.inst.mkNetwork.Add(ctx, *r.CellularRxBytes, metric.WithAttributes(attribute.String("direction", "rx"), attribute.String("transport", "cellular")))
	}
	if r.PeakMemoryBytes != nil && *r.PeakMemoryBytes > 0 {
		g.inst.mkMemoryPeak.Record(ctx, float64(*r.PeakMemoryBytes))
	}
	if r.ForegroundSeconds != nil {
		g.inst.mkAppTime.Add(ctx, *r.ForegroundSeconds, metric.WithAttributes(attribute.String("state", "foreground")))
	}
	if r.BackgroundSeconds != nil {
		g.inst.mkAppTime.Add(ctx, *r.BackgroundSeconds, metric.WithAttributes(attribute.String("state", "background")))
	}
}
