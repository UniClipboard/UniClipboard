package scenarios

import (
	"context"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
	"github.com/uniclipboard/unic/tools/bench/internal/report"
	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

// Run executes the requested benchmark scenarios and writes report.json,
// report.md, and SHA256SUMS into opts.OutputDir.
func Run(ctx context.Context, opts Options) (*report.Report, error) {
	if len(opts.Suite) == 0 {
		opts.Suite = AllScenarioIDs()
	}
	if opts.Repetitions < 1 {
		opts.Repetitions = 1
	}
	if opts.OutputDir == "" {
		opts.OutputDir = filepath.Join(".build", "bench", time.Now().UTC().Format("20060102-150405"))
	}
	absOut, err := filepath.Abs(opts.OutputDir)
	if err != nil {
		return nil, err
	}
	opts.OutputDir = absOut
	if err := os.MkdirAll(opts.OutputDir, 0o755); err != nil {
		return nil, fmt.Errorf("create output dir: %w", err)
	}
	if opts.RepoRoot == "" {
		if root, rerr := FindRepoRoot(""); rerr == nil {
			opts.RepoRoot = root
		}
	}

	bins, err := PrepareBinaries(ctx, &opts)
	if err != nil {
		return nil, err
	}

	host := sampler.CollectHostInfo(ctx)
	commit, dirty := collectGitInfo(opts.RepoRoot)
	clipBefore, clipErr := sampler.ClipboardSequence()

	rep := &report.Report{
		SchemaVersion:  report.SchemaVersion,
		RunID:          filepath.Base(opts.OutputDir),
		GeneratedAtUTC: time.Now().UTC().Format(time.RFC3339),
		Track:          string(opts.Track),
		Preset:         joinPresets(opts.Datasets),
		Environment: report.EnvironmentInfo{
			OS:                  host.OS,
			HostArch:            host.HostArch,
			OSVersion:           host.OSVersion,
			CPUModel:            host.CPUModel,
			LogicalCPUs:         host.LogicalCPUs,
			PhysicalMemoryBytes: host.PhysicalMemoryBytes,
			VirtualMachine:      host.VirtualMachine,
			Hypervisor:          host.Hypervisor,
			GoVersion:           runtime.Version(),
			GitCommit:           commit,
			GitDirty:            dirty,
		},
		Binaries:       bins,
		DerivedMetrics: map[string]float64{},
	}

	// Generate deterministic corpora once per dataset in opts.OutputDir/fixtures/<preset>.
	manifests := map[fixture.Preset]*fixture.Manifest{}
	fixtureDirs := map[fixture.Preset]string{}
	for _, ds := range opts.Datasets {
		fdir := filepath.Join(opts.OutputDir, "fixtures", string(ds))
		m, err := fixture.Generate(fdir, ds)
		if err != nil {
			return nil, fmt.Errorf("generate fixture %s: %w", ds, err)
		}
		manifests[ds] = m
		fixtureDirs[ds] = fdir
		// Use the primary (last/largest) dataset as report.Fixture summary.
		rep.Fixture = report.FixtureInfo{
			Preset:         string(ds),
			Seed:           m.Seed,
			ManifestSHA256: m.ManifestSHA256,
			TextCount:      m.TextCount,
			ImageCount:     m.ImageCount,
			FileCount:      m.FileCount,
		}
	}

	timings := TimingsForProfile(opts.Profile)
	runRunner := &runner{
		opts:        opts,
		timings:     timings,
		manifests:   manifests,
		fixtureDirs: fixtureDirs,
	}

	for _, scID := range opts.Suite {
		results, err := runRunner.executeScenario(ctx, scID)
		if err != nil {
			rep.Scenarios = append(rep.Scenarios, report.ScenarioResult{
				ID:     scID,
				Track:  string(opts.Track),
				Preset: string(opts.Profile),
				Status: "FAIL",
				Error:  err.Error(),
			})
			continue
		}
		rep.Scenarios = append(rep.Scenarios, results...)
	}

	// Compute cross-scenario derived metrics (e.g. normal vs lightweight net delta).
	computeReportDerivedMetrics(rep)

	clipAfter, clipAfterErr := sampler.ClipboardSequence()
	if clipErr == nil && clipAfterErr == nil {
		rep.ClipboardIntegrity = report.ClipboardCheck{
			Available:      true,
			BeforeSequence: clipBefore,
			AfterSequence:  clipAfter,
			Unchanged:      clipBefore == clipAfter,
		}
		if !rep.ClipboardIntegrity.Unchanged {
			rep.Scenarios = append(rep.Scenarios, report.ScenarioResult{
				ID:     "clipboard-integrity",
				Track:  string(opts.Track),
				Preset: rep.Preset,
				Status: "FAIL",
				Error:  fmt.Sprintf("system clipboard sequence changed across run: %d -> %d", clipBefore, clipAfter),
			})
		}
	} else {
		rep.ClipboardIntegrity = report.ClipboardCheck{
			Available: false,
			Unchanged: true,
			Note:      fmt.Sprintf("clipboard sequence unavailable: before=%v after=%v", clipErr, clipAfterErr),
		}
	}

	jsonPath := filepath.Join(opts.OutputDir, "report.json")
	if err := report.WriteJSON(jsonPath, rep); err != nil {
		return nil, err
	}
	mdPath := filepath.Join(opts.OutputDir, "report.md")
	if err := os.WriteFile(mdPath, []byte(report.RenderMarkdown(rep)), 0o600); err != nil {
		return nil, err
	}
	if _, err := report.WriteSHA256SUMS(opts.OutputDir, "report.json", "report.md"); err != nil {
		return nil, err
	}

	for _, sc := range rep.Scenarios {
		if sc.Status == "FAIL" {
			return rep, fmt.Errorf("one or more scenarios failed (first failure: %s: %s)", sc.ID, sc.Error)
		}
	}
	return rep, nil
}

type runner struct {
	opts        Options
	timings     Timings
	manifests   map[fixture.Preset]*fixture.Manifest
	fixtureDirs map[fixture.Preset]string
}

func (r *runner) logf(format string, args ...any) {
	if r.opts.Logf != nil {
		r.opts.Logf(format, args...)
	}
}

func (r *runner) hasDataset(p fixture.Preset) bool {
	_, ok := r.manifests[p]
	return ok
}

func (r *runner) preferredStandardDataset() fixture.Preset {
	for _, cand := range []fixture.Preset{fixture.PresetStandard, fixture.PresetSoak, fixture.PresetEmpty} {
		if r.hasDataset(cand) {
			return cand
		}
	}
	return r.opts.Datasets[0]
}

func (r *runner) executeScenario(ctx context.Context, id string) ([]report.ScenarioResult, error) {
	wantProd := r.opts.Track == TrackProduction || r.opts.Track == TrackBoth
	wantInter := r.opts.Track == TrackInteractive || r.opts.Track == TrackBoth

	switch id {
	case ScenarioBgIdleNormal:
		if !wantProd {
			return nil, nil
		}
		var out []report.ScenarioResult
		for _, ds := range r.opts.Datasets {
			res := r.runRepeated(ctx, id, string(TrackProduction), ds, r.opts.Repetitions, func(round int) (report.ScenarioResult, error) {
				return r.runBgIdleNormalOnce(ctx, ds, round)
			})
			out = append(out, res)
		}
		return out, nil

	case ScenarioBgIdleLightweight:
		if !wantProd {
			return nil, nil
		}
		ds := r.preferredStandardDataset()
		res := r.runRepeated(ctx, id, string(TrackProduction), ds, r.opts.Repetitions, func(round int) (report.ScenarioResult, error) {
			return r.runBgIdleLightweightOnce(ctx, ds, round)
		})
		return []report.ScenarioResult{res}, nil

	case ScenarioFgVisibleIdle:
		if !wantProd {
			return nil, nil
		}
		var out []report.ScenarioResult
		for _, ds := range r.opts.Datasets {
			res := r.runRepeated(ctx, id, string(TrackProduction), ds, r.opts.Repetitions, func(round int) (report.ScenarioResult, error) {
				return r.runFgVisibleIdleOnce(ctx, ds, round)
			})
			out = append(out, res)
		}
		return out, nil

	case ScenarioFgInteractive:
		if !wantInter {
			return nil, nil
		}
		ds := r.preferredStandardDataset()
		res := r.runFgInteractiveOnce(ctx, ds)
		return []report.ScenarioResult{res}, nil

	case ScenarioFgCloseReclaim:
		if !wantProd {
			return nil, nil
		}
		ds := r.preferredStandardDataset()
		res := r.runFgCloseReclaimOnce(ctx, ds)
		return []report.ScenarioResult{res}, nil

	case ScenarioQpLifecycle:
		ds := r.preferredStandardDataset()
		var out []report.ScenarioResult
		if wantProd {
			out = append(out, r.runQpLifecycleOnce(ctx, TrackProduction, ds))
		}
		if wantInter {
			out = append(out, r.runQpLifecycleOnce(ctx, TrackInteractive, ds))
		}
		return out, nil

	default:
		return nil, fmt.Errorf("unsupported scenario %q", id)
	}
}

func (r *runner) runRepeated(ctx context.Context, id, track string, ds fixture.Preset, reps int, fn func(round int) (report.ScenarioResult, error)) report.ScenarioResult {
	var roundResults []report.ScenarioResult
	for i := 1; i <= reps; i++ {
		r.logf("running %s [track=%s dataset=%s round=%d/%d]", id, track, ds, i, reps)
		res, err := fn(i)
		if err != nil {
			r.logf("FAILED %s [track=%s dataset=%s round=%d]: %v", id, track, ds, i, err)
			return report.ScenarioResult{
				ID:      id,
				Dataset: string(ds),
				Track:   track,
				Preset:  string(r.opts.Profile),
				Rounds:  i,
				Status:  "FAIL",
				Error:   fmt.Sprintf("round %d: %v", i, err),
			}
		}
		if res.Status == "FAIL" {
			r.logf("FAILED %s [track=%s dataset=%s round=%d]: %s", id, track, ds, i, res.Error)
			return res
		}
		roundResults = append(roundResults, res)
		if ctx.Err() != nil {
			break
		}
	}
	return AggregateMedianRound(roundResults)
}

// AggregateMedianRound selects the median round by steady-state primary memory
// and summarizes across repetitions so transient startup jitter does not skew results (F3).
func AggregateMedianRound(rounds []report.ScenarioResult) report.ScenarioResult {
	if len(rounds) == 0 {
		return report.ScenarioResult{Status: "FAIL", Error: "no rounds executed"}
	}
	if len(rounds) == 1 {
		rounds[0].Rounds = 1
		return rounds[0]
	}
	type indexed struct {
		idx int
		mem uint64
	}
	vals := make([]indexed, 0, len(rounds))
	for i, rr := range rounds {
		var m uint64
		if len(rr.Phases) > 0 {
			m = rr.Phases[len(rr.Phases)-1].SteadyMedianBytes
		}
		vals = append(vals, indexed{idx: i, mem: m})
	}
	sort.Slice(vals, func(i, j int) bool { return vals[i].mem < vals[j].mem })
	chosen := rounds[vals[len(vals)/2].idx]
	chosen.Rounds = len(rounds)
	chosen.Notes = append(chosen.Notes, fmt.Sprintf("Selected median round across %d repetitions by steady_median_bytes.", len(rounds)))
	return chosen
}

func (r *runner) sampleWindow(ctx context.Context, phase string, rolePIDs map[string]int, expectedRoles []string, dur time.Duration) ([]sampler.GroupSample, error) {
	interval := r.timings.SampleInterval
	if interval <= 0 {
		interval = time.Second
	}
	start := time.Now()
	var samples []sampler.GroupSample
	idx := 0
	for {
		elapsed := time.Since(start)
		gs, err := sampler.SampleGroup(idx, phase, elapsed.Seconds(), rolePIDs, expectedRoles)
		if err != nil {
			return samples, err
		}
		samples = append(samples, gs)
		idx++
		if elapsed >= dur {
			break
		}
		sleepFor := interval
		if rem := dur - elapsed; rem < sleepFor {
			sleepFor = rem
		}
		select {
		case <-ctx.Done():
			return samples, ctx.Err()
		case <-time.After(sleepFor):
		}
	}
	return samples, nil
}

type bgSampler struct {
	mu      sync.Mutex
	samples []sampler.GroupSample
	err     error
	cancel  context.CancelFunc
	done    chan struct{}
}

func startBackgroundSampler(ctx context.Context, phase string, rolePIDs map[string]int, expectedRoles []string, interval time.Duration) *bgSampler {
	sctx, cancel := context.WithCancel(ctx)
	b := &bgSampler{cancel: cancel, done: make(chan struct{})}
	go func() {
		defer close(b.done)
		start := time.Now()
		idx := 0
		for {
			gs, err := sampler.SampleGroup(idx, phase, time.Since(start).Seconds(), rolePIDs, expectedRoles)
			b.mu.Lock()
			if err != nil && b.err == nil {
				b.err = err
			}
			if err == nil {
				b.samples = append(b.samples, gs)
			}
			b.mu.Unlock()
			idx++
			select {
			case <-sctx.Done():
				return
			case <-time.After(interval):
			}
		}
	}()
	return b
}

func (b *bgSampler) stop() ([]sampler.GroupSample, error) {
	b.cancel()
	<-b.done
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.samples, b.err
}

func (r *runner) checkClipBefore(res *report.ScenarioResult) (uint64, bool, error) {
	seq, err := sampler.ClipboardSequence()
	if err != nil {
		return 0, false, nil
	}
	return seq, true, nil
}

func verifyClipAfter(before uint64, available bool, res *report.ScenarioResult) {
	if !available {
		return
	}
	after, err := sampler.ClipboardSequence()
	if err != nil {
		res.Status = "FAIL"
		res.Error = fmt.Sprintf("clipboard sequence check failed: %v", err)
		return
	}
	if after != before {
		res.Status = "FAIL"
		res.Error = fmt.Sprintf("clipboard sequence mutated during scenario: %d -> %d (F9)", before, after)
	}
}

func (r *runner) runBgIdleNormalOnce(ctx context.Context, ds fixture.Preset, round int) (report.ScenarioResult, error) {
	res := report.ScenarioResult{
		ID:            ScenarioBgIdleNormal,
		Dataset:       string(ds),
		Track:         string(TrackProduction),
		Preset:        string(r.opts.Profile),
		Status:        "PASS",
		ExpectedRoles: []string{"unicd", "tray", "quick-panel"},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	sess, err := newBenchSession(ctx, r.opts.Binaries.Unic, r.opts.Binaries.Unicd, r.opts.Binaries.Desktop, "normal", false, "")
	if err != nil {
		return res, err
	}
	defer func() {
		if terr := sess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()

	if err := sess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		return res, err
	}
	rolePIDs, err := sess.startNormalBackground(ctx)
	if err != nil {
		return res, err
	}

	totalDur := r.timings.BgWarmup + r.timings.BgSteady
	samples, err := r.sampleWindow(ctx, "steady", rolePIDs, res.ExpectedRoles, totalDur)
	if err != nil {
		return res, err
	}
	sum := sampler.SummarizePhase("steady", samples, res.ExpectedRoles, r.timings.BgWarmup.Seconds(), r.timings.BgWarmup.Seconds())
	res.Phases = []sampler.PhaseSummary{sum}

	if attr, aerr := sampler.CaptureAttribution(ctx, "steady", rolePIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	verifyClipAfter(clipBefore, clipAvail, &res)
	_ = round
	return res, nil
}

func (r *runner) runBgIdleLightweightOnce(ctx context.Context, ds fixture.Preset, round int) (report.ScenarioResult, error) {
	res := report.ScenarioResult{
		ID:            ScenarioBgIdleLightweight,
		Dataset:       string(ds),
		Track:         string(TrackProduction),
		Preset:        string(r.opts.Profile),
		Status:        "PASS",
		ExpectedRoles: []string{"unicd"},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	sess, err := newBenchSession(ctx, r.opts.Binaries.Unic, r.opts.Binaries.Unicd, r.opts.Binaries.Desktop, "lightweight", false, "")
	if err != nil {
		return res, err
	}
	defer func() {
		if terr := sess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()

	if err := sess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		return res, err
	}
	rolePIDs, err := sess.startLightweightBackground(ctx)
	if err != nil {
		return res, err
	}

	totalDur := r.timings.BgWarmup + r.timings.BgSteady
	samples, err := r.sampleWindow(ctx, "steady", rolePIDs, res.ExpectedRoles, totalDur)
	if err != nil {
		return res, err
	}
	sum := sampler.SummarizePhase("steady", samples, res.ExpectedRoles, r.timings.BgWarmup.Seconds(), r.timings.BgWarmup.Seconds())
	res.Phases = []sampler.PhaseSummary{sum}

	if attr, aerr := sampler.CaptureAttribution(ctx, "steady", rolePIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	verifyClipAfter(clipBefore, clipAvail, &res)
	_ = round
	return res, nil
}

func (r *runner) runFgVisibleIdleOnce(ctx context.Context, ds fixture.Preset, round int) (report.ScenarioResult, error) {
	res := report.ScenarioResult{
		ID:            ScenarioFgVisibleIdle,
		Dataset:       string(ds),
		Track:         string(TrackProduction),
		Preset:        string(r.opts.Profile),
		Status:        "PASS",
		ExpectedRoles: []string{"unicd", "tray", "quick-panel", "gui"},
		Latencies:     map[string]report.LatencyMetric{},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	sess, err := newBenchSession(ctx, r.opts.Binaries.Unic, r.opts.Binaries.Unicd, r.opts.Binaries.Desktop, "normal", false, "")
	if err != nil {
		return res, err
	}
	defer func() {
		if terr := sess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()

	if err := sess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		return res, err
	}
	rolePIDs, err := sess.startNormalBackground(ctx)
	if err != nil {
		return res, err
	}
	if err := waitDaemonConverged(ctx, rolePIDs["unicd"], 42*time.Second, 70*time.Second); err != nil {
		return res, err
	}
	guiPID, visMs, err := sess.openGUI(ctx)
	if err != nil {
		return res, err
	}
	rolePIDs["gui"] = guiPID
	res.Latencies["ipc_to_visible_ms"] = summarizeLatency("ipc_to_visible_ms", []float64{visMs}, "IPC open-gui dispatch to role status visible=true (excludes OS compositor scanout).")

	totalDur := r.timings.FgWarmup + r.timings.FgSteady
	samples, err := r.sampleWindow(ctx, "fg-visible-idle", rolePIDs, res.ExpectedRoles, totalDur)
	if err != nil {
		return res, err
	}
	sum := sampler.SummarizePhase("fg-visible-idle", samples, res.ExpectedRoles, r.timings.FgWarmup.Seconds(), r.timings.FgWarmup.Seconds())
	res.Phases = []sampler.PhaseSummary{sum}

	if attr, aerr := sampler.CaptureAttribution(ctx, "fg-visible-idle", rolePIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	verifyClipAfter(clipBefore, clipAvail, &res)
	_ = round
	return res, nil
}

func (r *runner) runFgInteractiveOnce(ctx context.Context, ds fixture.Preset) report.ScenarioResult {
	metricsDir := filepath.Join(r.opts.OutputDir, "probe-metrics", ScenarioFgInteractive+"-"+string(ds))
	_ = os.RemoveAll(metricsDir)
	res := report.ScenarioResult{
		ID:             ScenarioFgInteractive,
		Dataset:        string(ds),
		Track:          string(TrackInteractive),
		Preset:         string(r.opts.Profile),
		Rounds:         1,
		Status:         "PASS",
		ExpectedRoles:  []string{"unicd", "tray", "quick-panel", "gui"},
		Latencies:      map[string]report.LatencyMetric{},
		DerivedMetrics: map[string]float64{},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	sess, err := newBenchSession(ctx, r.opts.Binaries.Unic, r.opts.Binaries.UnicdProbe, r.opts.Binaries.DesktopE2EProbe, "normal", true, metricsDir)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	defer func() {
		if terr := sess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()

	if err := sess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	rolePIDs, err := sess.startNormalBackground(ctx)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	if err := waitDaemonConverged(ctx, rolePIDs["unicd"], 42*time.Second, 70*time.Second); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}

	guiPID, visMs, err := sess.openGUI(ctx)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	rolePIDs["gui"] = guiPID
	res.Latencies["ipc_to_visible_ms"] = summarizeLatency("ipc_to_visible_ms", []float64{visMs}, "IPC open-gui dispatch to visible=true")

	auto := &automationClient{socket: filepath.Join(sess.profile.HomeDir, "automation.sock")}
	winID, err := auto.WaitSearchReady(ctx, "")
	if err != nil {
		res.Status, res.Error = "FAIL", fmt.Sprintf("F7 gui automation readiness: %v", err)
		return res
	}
	_ = auto.Idle(ctx, winID, 5000)

	bgSamp := startBackgroundSampler(ctx, "fg-interactive-active", rolePIDs, res.ExpectedRoles, r.timings.SampleInterval)
	opTimes := make([]float64, 0, 10)
	var origTimes []float64

	// 1. Scroll list to exercise paging and thumbnail prefetch.
	t0 := time.Now()
	nodes, err := auto.Tree(ctx, winID)
	if err == nil {
		if listNode, ok := findNodeByRoleAndLabels(nodes, "list", "History items", "历史条目"); ok {
			cx := listNode.Bounds.X + listNode.Bounds.Width/2
			cy := listNode.Bounds.Y + listNode.Bounds.Height/2
			for step := 0; step < 4; step++ {
				_ = auto.Input(ctx, winID, []map[string]any{{"type": "wheel", "x": cx, "y": cy, "deltaY": -400}})
				time.Sleep(180 * time.Millisecond)
			}
			_ = auto.Idle(ctx, winID, 4000)
		}
	}
	opTimes = append(opTimes, float64(time.Since(t0).Milliseconds()))

	// 2. Toggle Grid view.
	t0 = time.Now()
	nodes, _ = auto.Tree(ctx, winID)
	if gridBtn, ok := findNodeByRoleAndLabels(nodes, "toggleButton", "Grid", "网格"); ok {
		_ = auto.Act(ctx, winID, gridBtn.ID, "press", "")
		_ = auto.Idle(ctx, winID, 4000)
	}
	opTimes = append(opTimes, float64(time.Since(t0).Milliseconds()))

	// 3. Search & filter images.
	t0 = time.Now()
	nodes, _ = auto.Tree(ctx, winID)
	if searchNode, ok := findNodeByRoleAndLabels(nodes, "comboBox", "Search history", "搜索历史"); ok {
		_ = auto.Act(ctx, winID, searchNode.ID, "setValue", "Synthetic")
		time.Sleep(450 * time.Millisecond)
		_ = auto.Idle(ctx, winID, 4000)
		_ = auto.Act(ctx, winID, searchNode.ID, "setValue", "")
		time.Sleep(450 * time.Millisecond)
		_ = auto.Idle(ctx, winID, 4000)
	}
	nodes, _ = auto.Tree(ctx, winID)
	if filterBtn, ok := findNodeByRoleAndLabels(nodes, "button", "Filters", "筛选条件"); ok {
		_ = auto.Act(ctx, winID, filterBtn.ID, "press", "")
		time.Sleep(250 * time.Millisecond)
		nodes, _ = auto.Tree(ctx, winID)
		if imgChip, ok := findNodeByRoleAndLabels(nodes, "", "Images", "图片"); ok {
			_ = auto.Act(ctx, winID, imgChip.ID, "press", "")
			time.Sleep(450 * time.Millisecond)
			_ = auto.Idle(ctx, winID, 4000)
		}
	}
	opTimes = append(opTimes, float64(time.Since(t0).Milliseconds()))

	// Switch back to List view so rows are deterministic for selection.
	nodes, _ = auto.Tree(ctx, winID)
	if listBtn, ok := findNodeByRoleAndLabels(nodes, "toggleButton", "List", "列表"); ok {
		_ = auto.Act(ctx, winID, listBtn.ID, "press", "")
		_ = auto.Idle(ctx, winID, 4000)
	}

	// 4. Scroll back to top and select up to 3 2K PNGs (skipping index 0 if >=4 exist
	// because index 0 is pre-selected on launch) to measure original decode readiness.
	nodes, _ = auto.Tree(ctx, winID)
	if listNode, ok := findNodeByRoleAndLabels(nodes, "list", "History items", "历史条目"); ok {
		cx := listNode.Bounds.X + listNode.Bounds.Width/2
		cy := listNode.Bounds.Y + listNode.Bounds.Height/2
		for step := 0; step < 5; step++ {
			_ = auto.Input(ctx, winID, []map[string]any{{"type": "wheel", "x": cx, "y": cy, "deltaY": 600}})
			time.Sleep(100 * time.Millisecond)
		}
		_ = auto.Idle(ctx, winID, 3000)
	}
	items, _ := sess.listHistory(ctx, 250)
	var imageIDs []string
	for _, it := range items {
		if it.Kind == "image" {
			imageIDs = append(imageIDs, it.ID)
		}
	}
	if len(imageIDs) > 3 {
		imageIDs = imageIDs[1:4]
	}
	nodes, treeErr := auto.Tree(ctx, winID)
	if treeErr != nil {
		res.Notes = append(res.Notes, fmt.Sprintf("gui tree read warning before image selection: %v", treeErr))
	}
	var targetRows []autoNode
	for _, imgID := range imageIDs {
		if row, ok := findNodeByKey(nodes, "listItem", imgID); ok {
			targetRows = append(targetRows, row)
		}
	}
	if len(targetRows) == 0 {
		for _, n := range nodes {
			if n.Role == "listItem" && (strings.Contains(n.Label, "Image") || strings.Contains(n.Label, "图片") || strings.Contains(n.Label, "2048")) {
				targetRows = append(targetRows, n)
			}
		}
		if len(targetRows) > 3 {
			targetRows = targetRows[1:4]
		}
	}
	preGuiEvs, _ := ReadOriginalReadyEvents(metricsDir, "gui", guiPID, 0)
	guiLastSeq, guiLastSelSeq := LatestEventCursor(preGuiEvs)
	for _, row := range targetRows {
		atMs := time.Now().UTC().UnixMilli()
		if ierr := auto.Act(ctx, winID, row.ID, "press", ""); ierr != nil {
			res.Notes = append(res.Notes, fmt.Sprintf("gui image selection press error: %v", ierr))
			continue
		}
		ev, werr := waitNextOriginalReadyEvent(ctx, metricsDir, "gui", guiPID, guiLastSeq, guiLastSelSeq, atMs, 12*time.Second, auto, winID)
		if werr != nil {
			res.Notes = append(res.Notes, fmt.Sprintf("gui image selection original ready timeout: %v", werr))
			continue
		}
		guiLastSeq, guiLastSelSeq = ev.Seq, ev.SelectionSeq
		origTimes = append(origTimes, float64(max(int64(1), ev.UnixMillis-atMs)))
	}

	// 5. Switch to Manage devices, Settings, and back to History.
	t0 = time.Now()
	nodes, _ = auto.Tree(ctx, winID)
	if devItem, ok := findNodeByRoleAndLabels(nodes, "listItem", "Manage devices…", "管理设备…"); ok {
		_ = auto.Act(ctx, winID, devItem.ID, "press", "")
		_ = auto.Idle(ctx, winID, 4000)
	}
	nodes, _ = auto.Tree(ctx, winID)
	if setItem, ok := findNodeByRoleAndLabels(nodes, "listItem", "Settings…", "设置…"); ok {
		_ = auto.Act(ctx, winID, setItem.ID, "press", "")
		_ = auto.Idle(ctx, winID, 4000)
	}
	nodes, _ = auto.Tree(ctx, winID)
	if backBtn, ok := findNodeByRoleAndLabels(nodes, "", "Back to history", "返回历史"); ok {
		_ = auto.Act(ctx, winID, backBtn.ID, "press", "")
		_ = auto.Idle(ctx, winID, 4000)
	}
	opTimes = append(opTimes, float64(time.Since(t0).Milliseconds()))

	activeSamples, activeErr := bgSamp.stop()
	if activeErr != nil {
		res.Status, res.Error = "FAIL", activeErr.Error()
		return res
	}
	activeSum := sampler.SummarizePhase("fg-interactive-active", activeSamples, res.ExpectedRoles, 0, 0)

	// 6. Post-interaction rest (15s) to measure recovery.
	settleSamples, settleErr := r.sampleWindow(ctx, "fg-interactive-settle", rolePIDs, res.ExpectedRoles, r.timings.FgInteractiveSettle)
	if settleErr != nil {
		res.Status, res.Error = "FAIL", settleErr.Error()
		return res
	}
	settleSum := sampler.SummarizePhase("fg-interactive-settle", settleSamples, res.ExpectedRoles, 0, 0)

	res.Phases = []sampler.PhaseSummary{activeSum, settleSum}
	if len(opTimes) > 0 {
		res.Latencies["interactive_op_ms"] = summarizeLatency("interactive_op_ms", opTimes, "UI interactive step durations")
	}
	if len(origTimes) > 0 {
		res.Latencies["ipc_to_original_ready_ms"] = summarizeLatency("ipc_to_original_ready_ms", origTimes, "GUI image row selection to decoded original ready")
	}
	if attr, aerr := sampler.CaptureAttribution(ctx, "fg-interactive-settle", rolePIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	if probeSum, perr := ParseRuntimeMetricsDir(metricsDir); perr == nil && len(probeSum) > 0 {
		res.ProbeByRole = probeSum
	}
	res.DerivedMetrics["peak_primary_memory_bytes"] = float64(activeSum.PeakPrimaryMemoryBytes)
	res.DerivedMetrics["post_rest_median_bytes"] = float64(settleSum.SteadyMedianBytes)
	verifyClipAfter(clipBefore, clipAvail, &res)
	return res
}

func (r *runner) runFgCloseReclaimOnce(ctx context.Context, ds fixture.Preset) report.ScenarioResult {
	res := report.ScenarioResult{
		ID:             ScenarioFgCloseReclaim,
		Dataset:        string(ds),
		Track:          string(TrackProduction),
		Preset:         string(r.opts.Profile),
		Rounds:         r.timings.FgCloseRounds,
		Status:         "PASS",
		ExpectedRoles:  []string{"unicd", "tray", "quick-panel"},
		Latencies:      map[string]report.LatencyMetric{},
		DerivedMetrics: map[string]float64{},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	sess, err := newBenchSession(ctx, r.opts.Binaries.Unic, r.opts.Binaries.Unicd, r.opts.Binaries.Desktop, "normal", false, "")
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	defer func() {
		if terr := sess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()

	if err := sess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	bgPIDs, err := sess.startNormalBackground(ctx)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	if err := waitDaemonConverged(ctx, bgPIDs["unicd"], 42*time.Second, 70*time.Second); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}

	// Pre-open baseline after unicd convergence (15s window with 5s warmup trimmed).
	baseSamples, err := r.sampleWindow(ctx, "pre-open-bg", bgPIDs, []string{"unicd", "tray", "quick-panel"}, 15*time.Second)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	baseSum := sampler.SummarizePhase("pre-open-bg", baseSamples, []string{"unicd", "tray", "quick-panel"}, 5, 5)
	res.Phases = append(res.Phases, baseSum)

	// Sub-scenario A: 5 rounds of open-gui -> hold 10s -> close -> verify gui exit -> rest 15s.
	var openLatencies []float64
	var openMedians []uint64
	var postCloseMedians []uint64
	var postCloseAll []sampler.GroupSample

	for round := 1; round <= r.timings.FgCloseRounds; round++ {
		guiPID, visMs, oerr := sess.openGUI(ctx)
		if oerr != nil {
			res.Status, res.Error = "FAIL", fmt.Sprintf("close-reclaim round %d open: %v", round, oerr)
			return res
		}
		openLatencies = append(openLatencies, visMs)

		fgPIDs := map[string]int{"unicd": bgPIDs["unicd"], "tray": bgPIDs["tray"], "quick-panel": bgPIDs["quick-panel"], "gui": guiPID}
		fgSamples, ferr := r.sampleWindow(ctx, fmt.Sprintf("close-cycle-%d-open", round), fgPIDs, []string{"unicd", "tray", "quick-panel", "gui"}, r.timings.FgCloseHold)
		if ferr != nil {
			res.Status, res.Error = "FAIL", ferr.Error()
			return res
		}
		fgSum := sampler.SummarizePhase(fmt.Sprintf("close-cycle-%d-open", round), fgSamples, []string{"unicd", "tray", "quick-panel", "gui"}, 0, 0)
		openMedians = append(openMedians, fgSum.SteadyMedianBytes)

		if cerr := sess.closeGUI(ctx); cerr != nil {
			res.Status, res.Error = "FAIL", fmt.Sprintf("close-reclaim round %d close: %v", round, cerr)
			return res
		}

		restSamples, rerr := r.sampleWindow(ctx, "post-close-bg", bgPIDs, []string{"unicd", "tray", "quick-panel"}, r.timings.FgCloseRest)
		if rerr != nil {
			res.Status, res.Error = "FAIL", rerr.Error()
			return res
		}
		postCloseAll = append(postCloseAll, restSamples...)
		restSum := sampler.SummarizePhase("post-close-bg", restSamples, []string{"unicd", "tray", "quick-panel"}, 0, 0)
		postCloseMedians = append(postCloseMedians, restSum.SteadyMedianBytes)
	}

	postCloseSum := sampler.SummarizePhase("post-close-bg", postCloseAll, []string{"unicd", "tray", "quick-panel"}, 0, 0)
	slope := ComputeRoundSlope(postCloseMedians)
	postCloseSum.LeakSlopeBytesPerRound = slope
	res.Phases = append(res.Phases, postCloseSum)
	res.LeakCheck = &report.LeakSummary{
		Metric:             "post_close_bg_primary_memory",
		Rounds:             len(postCloseMedians),
		SlopeBytesPerRound: slope,
		ValuesBytes:        postCloseMedians,
		Passed:             slope <= (1 << 20),
	}
	res.Latencies["ipc_to_visible_ms"] = summarizeLatency("ipc_to_visible_ms", openLatencies, "GUI open latency across close-reclaim rounds")

	if len(openMedians) > 0 && len(postCloseMedians) > 0 {
		openMed := medianUint64(openMedians)
		closeMed := medianUint64(postCloseMedians)
		stats := ComputeReclaimStats(baseSum.SteadyMedianBytes, openMed, closeMed)
		res.DerivedMetrics["baseline_median_bytes"] = float64(baseSum.SteadyMedianBytes)
		res.DerivedMetrics["open_median_bytes"] = float64(openMed)
		res.DerivedMetrics["open_spike_bytes"] = float64(stats.OpenSpikeBytes)
		res.DerivedMetrics["post_close_median_bytes"] = float64(closeMed)
		res.DerivedMetrics["reclaimed_bytes"] = float64(stats.ReclaimedBytes)
		res.DerivedMetrics["residual_above_baseline_bytes"] = float64(stats.ResidualAboveBaselineBytes)
		res.DerivedMetrics["reclaim_ratio_pct"] = stats.RatioPct
	}

	// Sub-scenario B: Hide/Minimize retention observation (20s hidden -> restore).
	guiPID, _, oerr := sess.openGUI(ctx)
	if oerr != nil {
		res.Status, res.Error = "FAIL", fmt.Sprintf("hide-retention open: %v", oerr)
		return res
	}
	fgPIDs := map[string]int{"unicd": bgPIDs["unicd"], "tray": bgPIDs["tray"], "quick-panel": bgPIDs["quick-panel"], "gui": guiPID}
	preHideSamples, err := r.sampleWindow(ctx, "pre-hide-visible", fgPIDs, []string{"unicd", "tray", "quick-panel", "gui"}, 5*time.Second)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	preHideSum := sampler.SummarizePhase("pre-hide-visible", preHideSamples, []string{"unicd", "tray", "quick-panel", "gui"}, 0, 0)
	res.Phases = append(res.Phases, preHideSum)

	if herr := hideGUIProcess(guiPID); herr != nil {
		res.Notes = append(res.Notes, fmt.Sprintf("hide/minimize skipped: %v", herr))
	} else {
		hiddenSamples, herr := r.sampleWindow(ctx, "gui-hidden-retained", fgPIDs, []string{"unicd", "tray", "quick-panel", "gui"}, r.timings.FgHideHold)
		if herr != nil {
			res.Status, res.Error = "FAIL", herr.Error()
			return res
		}
		hiddenSum := sampler.SummarizePhase("gui-hidden-retained", hiddenSamples, []string{"unicd", "tray", "quick-panel", "gui"}, 0, 0)
		res.Phases = append(res.Phases, hiddenSum)

		if rerr := restoreGUIProcess(guiPID); rerr == nil {
			_ = sess.roleCommand(ctx, "tray", "open-gui")
			restoredSamples, _ := r.sampleWindow(ctx, "gui-restored", fgPIDs, []string{"unicd", "tray", "quick-panel", "gui"}, 5*time.Second)
			if len(restoredSamples) > 0 {
				res.Phases = append(res.Phases, sampler.SummarizePhase("gui-restored", restoredSamples, []string{"unicd", "tray", "quick-panel", "gui"}, 0, 0))
			}
		}
		res.DerivedMetrics["hide_retention_delta_bytes"] = float64(int64(hiddenSum.SteadyMedianBytes) - int64(preHideSum.SteadyMedianBytes))
	}

	if attr, aerr := sampler.CaptureAttribution(ctx, "fg-close-reclaim", bgPIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	verifyClipAfter(clipBefore, clipAvail, &res)
	return res
}

func (r *runner) runQpLifecycleOnce(ctx context.Context, track Track, ds fixture.Preset) report.ScenarioResult {
	interactive := track == TrackInteractive
	metricsDir := ""
	unicdBin := r.opts.Binaries.Unicd
	desktopBin := r.opts.Binaries.Desktop
	if interactive {
		metricsDir = filepath.Join(r.opts.OutputDir, "probe-metrics", ScenarioQpLifecycle+"-"+string(ds))
		_ = os.RemoveAll(metricsDir)
		unicdBin = r.opts.Binaries.UnicdProbe
		desktopBin = r.opts.Binaries.DesktopE2EProbe
	}

	res := report.ScenarioResult{
		ID:             ScenarioQpLifecycle,
		Dataset:        string(ds),
		Track:          string(track),
		Preset:         string(r.opts.Profile),
		Rounds:         r.timings.QpWarmRounds,
		Status:         "PASS",
		ExpectedRoles:  []string{"unicd", "tray", "quick-panel"},
		Latencies:      map[string]report.LatencyMetric{},
		DerivedMetrics: map[string]float64{},
	}
	clipBefore, clipAvail, _ := r.checkClipBefore(&res)

	// Part 1: Cold 0033 Standalone Quick Panel lifecycle (unicd + --quick-panel).
	coldSess, err := newBenchSession(ctx, r.opts.Binaries.Unic, unicdBin, desktopBin, "normal", interactive, metricsDir)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	if err := coldSess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	unicdPID, err := coldSess.startDaemon(ctx)
	if err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}

	coldStartAt := time.Now()
	coldStartMs := coldStartAt.UTC().UnixMilli()
	if _, err := coldSess.runDesktop(ctx, "--quick-panel"); err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	qpInfo, _, err := coldSess.waitRoleReady(ctx, "quick-panel", true)
	if err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	coldVisMs := float64(time.Since(coldStartAt).Microseconds()) / 1000.0
	if runtime.GOOS == "windows" {
		if err := waitVisibleWindowsHWND(ctx, qpInfo.PID); err != nil {
			_ = coldSess.teardown(r.opts.KeepProfile)
			res.Status, res.Error = "FAIL", err.Error()
			return res
		}
	}
	var coldSearchMs []float64
	var coldOrigMs []float64
	if interactive {
		auto := &automationClient{socket: filepath.Join(coldSess.profile.HomeDir, "automation-panel.sock")}
		winID, err := auto.WaitSearchReady(ctx, "unic E2E")
		if err != nil {
			_ = coldSess.teardown(r.opts.KeepProfile)
			res.Status, res.Error = "FAIL", fmt.Sprintf("F7 cold quick-panel search ready: %v", err)
			return res
		}
		coldSearchMs = append(coldSearchMs, float64(time.Since(coldStartAt).Microseconds())/1000.0)
		ev, werr := waitNextOriginalReadyEvent(ctx, metricsDir, "quick-panel", qpInfo.PID, 0, 0, coldStartMs, 10*time.Second, auto, winID)
		if werr != nil {
			_ = coldSess.teardown(r.opts.KeepProfile)
			res.Status, res.Error = "FAIL", fmt.Sprintf("F7 cold quick-panel original ready: %v", werr)
			return res
		}
		coldOrigMs = append(coldOrigMs, float64(max(int64(1), ev.UnixMillis-coldStartMs)))
	}

	coldPIDs := map[string]int{"unicd": unicdPID, "quick-panel": qpInfo.PID}
	coldOpenSamples, err := r.sampleWindow(ctx, "qp-cold-open", coldPIDs, []string{"unicd", "quick-panel"}, r.timings.QpColdHold)
	if err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	res.Phases = append(res.Phases, sampler.SummarizePhase("qp-cold-open", coldOpenSamples, []string{"unicd", "quick-panel"}, 0, 0))

	if err := coldSess.closeQuickPanel(ctx); err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	coldStandbySamples, err := r.sampleWindow(ctx, "qp-cold-standby", coldPIDs, []string{"unicd", "quick-panel"}, r.timings.QpColdRest)
	if err != nil {
		_ = coldSess.teardown(r.opts.KeepProfile)
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	res.Phases = append(res.Phases, sampler.SummarizePhase("qp-cold-standby", coldStandbySamples, []string{"unicd", "quick-panel"}, 0, 0))
	if err := coldSess.teardown(r.opts.KeepProfile); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}

	// Part 2: Warm Normal-prewarmed cycles (10 rounds default / 20 soak).
	warmMetricsDir := ""
	if interactive {
		warmMetricsDir = filepath.Join(metricsDir, "warm")
	}
	warmSess, err := newBenchSession(ctx, r.opts.Binaries.Unic, unicdBin, desktopBin, "normal", interactive, warmMetricsDir)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	defer func() {
		if terr := warmSess.teardown(r.opts.KeepProfile); terr != nil && res.Status == "PASS" {
			res.Status = "FAIL"
			res.Error = terr.Error()
		}
	}()
	if err := warmSess.seed(ctx, r.fixtureDirs[ds], r.manifests[ds]); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	warmPIDs, err := warmSess.startNormalBackground(ctx)
	if err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}
	if err := waitDaemonConverged(ctx, warmPIDs["unicd"], 42*time.Second, 70*time.Second); err != nil {
		res.Status, res.Error = "FAIL", err.Error()
		return res
	}

	warmVisLat := make([]float64, 0, r.timings.QpWarmRounds)
	warmSearchLat := make([]float64, 0, r.timings.QpWarmRounds)
	warmOrigLat := make([]float64, 0, r.timings.QpWarmRounds*3)
	var warmOpenAll []sampler.GroupSample
	var warmClosedAll []sampler.GroupSample
	var postCycleMedians []uint64
	var postCycleHandles []uint32
	var postCycleFDs []int

	var auto *automationClient
	if interactive {
		auto = &automationClient{socket: filepath.Join(warmSess.profile.HomeDir, "automation-panel.sock")}
	}

	for round := 1; round <= r.timings.QpWarmRounds; round++ {
		var preOpenSeq uint64
		var openStartMs int64
		if interactive {
			preEvs, _ := ReadOriginalReadyEvents(warmMetricsDir, "quick-panel", warmPIDs["quick-panel"], 0)
			preOpenSeq, _ = LatestEventCursor(preEvs)
			openStartMs = time.Now().UTC().UnixMilli()
		}
		at := time.Now()
		qpPID, visMs, oerr := warmSess.openQuickPanelWarm(ctx)
		if oerr != nil {
			res.Status, res.Error = "FAIL", fmt.Sprintf("qp warm round %d open: %v", round, oerr)
			return res
		}
		warmPIDs["quick-panel"] = qpPID
		warmVisLat = append(warmVisLat, visMs)

		if interactive {
			winID, serr := auto.WaitSearchReady(ctx, "unic E2E")
			if serr != nil {
				res.Status, res.Error = "FAIL", fmt.Sprintf("F7 qp warm round %d search ready: %v", round, serr)
				return res
			}
			warmSearchLat = append(warmSearchLat, float64(time.Since(at).Microseconds())/1000.0)
			// Wait for initial @1 original preview ready before stepping Down.
			initEv, ierr := waitNextOriginalReadyEvent(ctx, warmMetricsDir, "quick-panel", qpPID, preOpenSeq, 0, openStartMs, 8*time.Second, auto, winID)
			if ierr != nil {
				res.Status, res.Error = "FAIL", fmt.Sprintf("F7 qp warm round %d initial original ready: %v", round, ierr)
				return res
			}
			stepSeq, stepSelSeq := initEv.Seq, initEv.SelectionSeq
			// Respect Quick Panel previewDue debounce before & between Down steps and settle frames.
			time.Sleep(150 * time.Millisecond)
			_ = auto.Idle(ctx, winID, 400)
			for step := 0; step < 3; step++ {
				if step > 0 {
					time.Sleep(150 * time.Millisecond)
				}
				stepAt := time.Now().UTC().UnixMilli()
				if ierr := auto.Input(ctx, winID, []map[string]any{{"type": "key", "key": "Down"}}); ierr != nil {
					res.Status, res.Error = "FAIL", fmt.Sprintf("F7 qp warm round %d step %d input Down: %v", round, step, ierr)
					return res
				}
				ev, werr := waitNextOriginalReadyEvent(ctx, warmMetricsDir, "quick-panel", qpPID, stepSeq, stepSelSeq, stepAt, 8*time.Second, auto, winID)
				if werr != nil {
					res.Status, res.Error = "FAIL", fmt.Sprintf("F7 qp warm round %d step %d original ready: %v", round, step, werr)
					return res
				}
				stepSeq, stepSelSeq = ev.Seq, ev.SelectionSeq
				warmOrigLat = append(warmOrigLat, float64(max(int64(1), ev.UnixMillis-stepAt)))
			}
		}

		openSamples, serr := r.sampleWindow(ctx, "qp-warm-open", warmPIDs, res.ExpectedRoles, r.timings.QpWarmHold)
		if serr != nil {
			res.Status, res.Error = "FAIL", serr.Error()
			return res
		}
		warmOpenAll = append(warmOpenAll, openSamples...)

		if cerr := warmSess.closeQuickPanel(ctx); cerr != nil {
			res.Status, res.Error = "FAIL", fmt.Sprintf("qp warm round %d close: %v", round, cerr)
			return res
		}
		closedSamples, rerr := r.sampleWindow(ctx, "qp-warm-closed", warmPIDs, res.ExpectedRoles, r.timings.QpWarmRest)
		if rerr != nil {
			res.Status, res.Error = "FAIL", rerr.Error()
			return res
		}
		warmClosedAll = append(warmClosedAll, closedSamples...)
		cycleSum := sampler.SummarizePhase("qp-warm-closed", closedSamples, res.ExpectedRoles, 0, 0)
		postCycleMedians = append(postCycleMedians, cycleSum.SteadyMedianBytes)
		if len(closedSamples) > 0 {
			last := closedSamples[len(closedSamples)-1]
			postCycleHandles = append(postCycleHandles, last.TotalHandles)
			postCycleFDs = append(postCycleFDs, last.TotalFDs)
		}
	}

	warmOpenSum := sampler.SummarizePhase("qp-warm-open", warmOpenAll, res.ExpectedRoles, 0, 0)
	warmClosedSum := sampler.SummarizePhase("qp-warm-closed", warmClosedAll, res.ExpectedRoles, 0, 0)
	slope := ComputeRoundSlope(postCycleMedians)
	warmClosedSum.LeakSlopeBytesPerRound = slope
	res.Phases = append(res.Phases, warmOpenSum, warmClosedSum)

	res.LeakCheck = &report.LeakSummary{
		Metric:             "qp_warm_post_close_primary_memory",
		Rounds:             len(postCycleMedians),
		SlopeBytesPerRound: slope,
		ValuesBytes:        postCycleMedians,
		Passed:             slope <= (1 << 20),
	}

	res.Latencies["cold_ipc_to_visible_ms"] = summarizeLatency("cold_ipc_to_visible_ms", []float64{coldVisMs}, "Standalone --quick-panel cold IPC launch to visible=true (excludes physical hotkey & compositor scanout)")
	res.Latencies["ipc_to_visible_ms"] = summarizeLatency("ipc_to_visible_ms", warmVisLat, "Prewarmed quick-panel IPC open to visible=true (excludes physical hotkey & compositor scanout)")
	if len(coldSearchMs) > 0 {
		res.Latencies["cold_ipc_to_search_ready_ms"] = summarizeLatency("cold_ipc_to_search_ready_ms", coldSearchMs, "Standalone --quick-panel cold IPC launch to search field ready")
	}
	if len(warmSearchLat) > 0 {
		res.Latencies["ipc_to_search_ready_ms"] = summarizeLatency("ipc_to_search_ready_ms", warmSearchLat, "Prewarmed quick-panel IPC open to search field ready")
	}
	allOrig := append(coldOrigMs, warmOrigLat...)
	if len(allOrig) > 0 {
		res.Latencies["ipc_to_original_ready_ms"] = summarizeLatency("ipc_to_original_ready_ms", allOrig, "Quick-panel original 2K image preview ready latency")
	}

	if len(postCycleMedians) >= 2 {
		res.DerivedMetrics["warm_post_close_first_bytes"] = float64(postCycleMedians[0])
		res.DerivedMetrics["warm_post_close_last_bytes"] = float64(postCycleMedians[len(postCycleMedians)-1])
		res.DerivedMetrics["warm_post_close_delta_bytes"] = float64(int64(postCycleMedians[len(postCycleMedians)-1]) - int64(postCycleMedians[0]))
	}
	if len(postCycleFDs) >= 2 {
		res.DerivedMetrics["warm_post_close_fds_delta"] = float64(postCycleFDs[len(postCycleFDs)-1] - postCycleFDs[0])
	}
	if len(postCycleHandles) >= 2 {
		res.DerivedMetrics["warm_post_close_handles_delta"] = float64(int64(postCycleHandles[len(postCycleHandles)-1]) - int64(postCycleHandles[0]))
	}

	if attr, aerr := sampler.CaptureAttribution(ctx, "qp-warm-closed", warmPIDs); aerr == nil && attr != nil {
		res.Attribution = append(res.Attribution, *attr)
	}
	if interactive {
		if probeSum, perr := ParseRuntimeMetricsDir(warmMetricsDir); perr == nil && len(probeSum) > 0 {
			res.ProbeByRole = probeSum
		}
	}
	verifyClipAfter(clipBefore, clipAvail, &res)
	return res
}

// ComputeRoundSlope computes OLS bytes/round slope across round-end memory values.
func ComputeRoundSlope(values []uint64) float64 {
	if len(values) < 2 {
		return 0
	}
	xs := make([]float64, len(values))
	ys := make([]float64, len(values))
	for i, v := range values {
		xs[i] = float64(i)
		ys[i] = float64(v)
	}
	slope, _ := sampler.LinearRegressionSlope(xs, ys)
	return slope
}

func waitNextOriginalReadyEvent(ctx context.Context, dir, role string, pid int, afterSeq, afterSelSeq uint64, minUnixMillis int64, timeout time.Duration, auto *automationClient, winID int) (probeEventRow, error) {
	wctx, cancel := context.WithTimeout(ctx, timeout)
	defer cancel()
	for {
		if auto != nil && winID > 0 {
			_ = auto.Idle(wctx, winID, 150)
		}
		evs, err := ReadOriginalReadyEvents(dir, role, pid, 0)
		if err == nil {
			if matched := FilterNewOriginalReadyEvents(evs, afterSeq, afterSelSeq, minUnixMillis); len(matched) > 0 {
				return matched[len(matched)-1], nil
			}
		}
		select {
		case <-wctx.Done():
			return probeEventRow{}, fmt.Errorf("timed out waiting for %s selected-original-ready (afterSeq=%d, afterSelSeq=%d): %w", role, afterSeq, afterSelSeq, wctx.Err())
		case <-time.After(40 * time.Millisecond):
		}
	}
}

// ReclaimStats describes foreground memory reclaim relative to background baseline.
type ReclaimStats struct {
	ReclaimedBytes             int64
	ResidualAboveBaselineBytes int64
	OpenSpikeBytes             int64
	RatioPct                   float64
}

// ComputeReclaimStats computes bounded reclaim ratio in [0, 100] alongside exact
// signed reclaim and post-close residual-above-baseline bytes.
func ComputeReclaimStats(baselineBytes, openBytes, postCloseBytes uint64) ReclaimStats {
	spike := int64(openBytes) - int64(baselineBytes)
	reclaimed := int64(openBytes) - int64(postCloseBytes)
	residual := int64(postCloseBytes) - int64(baselineBytes)

	var ratio float64
	switch {
	case spike <= 0:
		if postCloseBytes <= baselineBytes {
			ratio = 100.0
		} else {
			ratio = 0.0
		}
	case postCloseBytes <= baselineBytes || reclaimed >= spike:
		ratio = 100.0
	case reclaimed <= 0:
		ratio = 0.0
	default:
		ratio = (float64(reclaimed) / float64(spike)) * 100.0
	}
	return ReclaimStats{
		ReclaimedBytes:             reclaimed,
		ResidualAboveBaselineBytes: residual,
		OpenSpikeBytes:             spike,
		RatioPct:                   ratio,
	}
}

// ComputeReclaimRatio computes the bounded percentage [0, 100] of foreground spike reclaimed after closing GUI.
func ComputeReclaimRatio(baselineBytes, openBytes, postCloseBytes uint64) (ratioPct float64, reclaimedBytes int64) {
	st := ComputeReclaimStats(baselineBytes, openBytes, postCloseBytes)
	return st.RatioPct, st.ReclaimedBytes
}

func computeReportDerivedMetrics(rep *report.Report) {
	if rep == nil {
		return
	}
	if rep.DerivedMetrics == nil {
		rep.DerivedMetrics = map[string]float64{}
	}
	var norm, light *report.ScenarioResult
	for i := range rep.Scenarios {
		sc := &rep.Scenarios[i]
		if sc.Status != "PASS" || len(sc.Phases) == 0 {
			continue
		}
		if sc.ID == ScenarioBgIdleNormal && (sc.Dataset == string(fixture.PresetStandard) || norm == nil) {
			norm = sc
		}
		if sc.ID == ScenarioBgIdleLightweight {
			light = sc
		}
	}
	if norm != nil && light != nil && len(norm.Phases) > 0 && len(light.Phases) > 0 {
		nph := norm.Phases[0]
		lph := light.Phases[0]
		memDelta := float64(int64(nph.SteadyMedianBytes) - int64(lph.SteadyMedianBytes))
		cpuDelta := nph.CPUPercentMean - lph.CPUPercentMean
		rep.DerivedMetrics["tray_plus_standby_panel_primary_bytes"] = memDelta
		rep.DerivedMetrics["tray_plus_standby_panel_cpu_percent"] = cpuDelta
		if nph.PrivateCommitBytes.Count > 0 && lph.PrivateCommitBytes.Count > 0 {
			rep.DerivedMetrics["tray_plus_standby_panel_commit_bytes"] = nph.PrivateCommitBytes.Median - lph.PrivateCommitBytes.Median
		}
		light.DerivedMetrics = map[string]float64{
			"tray_plus_standby_panel_primary_bytes": memDelta,
			"tray_plus_standby_panel_cpu_percent":   cpuDelta,
		}
	}
}

func summarizeLatency(name string, vals []float64, note string) report.LatencyMetric {
	st := sampler.SummarizeValues(vals)
	return report.LatencyMetric{
		Name:      name,
		Count:     st.Count,
		P50Ms:     st.Median,
		P95Ms:     st.P95,
		P99Ms:     st.P99,
		MaxMs:     st.Max,
		SamplesMs: slices.Clone(vals),
		Note:      note,
	}
}

func medianUint64(vals []uint64) uint64 {
	if len(vals) == 0 {
		return 0
	}
	sorted := slices.Clone(vals)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i] < sorted[j] })
	mid := len(sorted) / 2
	if len(sorted)%2 == 1 {
		return sorted[mid]
	}
	return uint64(math.Round(float64(sorted[mid-1]+sorted[mid]) / 2.0))
}

func joinPresets(ds []fixture.Preset) string {
	parts := make([]string, len(ds))
	for i, d := range ds {
		parts[i] = string(d)
	}
	return strings.Join(parts, ",")
}
