package scenarios

import (
	"math"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
	"github.com/uniclipboard/unic/tools/bench/internal/report"
	"github.com/uniclipboard/unic/tools/bench/internal/sampler"
)

func TestParseScenariosTrackProfileDatasets(t *testing.T) {
	all, err := ParseScenarios("all")
	if err != nil || len(all) != 6 {
		t.Fatalf("ParseScenarios(all) = %v, err=%v", all, err)
	}
	sub, err := ParseScenarios("bg-idle-normal, qp-lifecycle, bg-idle-normal")
	if err != nil || len(sub) != 2 {
		t.Fatalf("ParseScenarios dedup = %v, err=%v", sub, err)
	}
	if _, err := ParseScenarios("unknown-scenario"); err == nil {
		t.Fatal("expected error for unknown scenario")
	}

	tr, err := ParseTrack("both")
	if err != nil || tr != TrackBoth {
		t.Fatalf("ParseTrack = %q, %v", tr, err)
	}
	pr, err := ParseProfile("soak")
	if err != nil || pr != ProfileSoak {
		t.Fatalf("ParseProfile = %q, %v", pr, err)
	}
	tim := TimingsForProfile(ProfileSoak)
	if tim.QpWarmRounds != 20 || tim.BgSteady.Seconds() != 300 {
		t.Fatalf("unexpected soak timings: %+v", tim)
	}

	ds, err := ParseDatasets("", ProfileDefault)
	if err != nil || len(ds) != 2 || ds[0] != fixture.PresetEmpty || ds[1] != fixture.PresetStandard {
		t.Fatalf("ParseDatasets default = %v, %v", ds, err)
	}
}

func TestDerivedMetricsAndLeakSlopes(t *testing.T) {
	ratio, reclaimed := ComputeReclaimRatio(80<<20, 180<<20, 90<<20)
	if reclaimed != (90<<20) || math.Abs(ratio-90.0) > 1e-6 {
		t.Fatalf("ComputeReclaimRatio = (%v, %d), want (90.0, %d)", ratio, reclaimed, int64(90<<20))
	}

	// Boundary cases: postClose below baseline, open <= baseline, and negative reclaim.
	belowBase := ComputeReclaimStats(110<<20, 121<<20, 78<<20)
	if belowBase.RatioPct != 100.0 || belowBase.ReclaimedBytes != int64(43<<20) || belowBase.ResidualAboveBaselineBytes != -int64(32<<20) {
		t.Fatalf("belowBase stats = %+v", belowBase)
	}
	noSpike := ComputeReclaimStats(100<<20, 95<<20, 110<<20)
	if noSpike.RatioPct != 0.0 || noSpike.ResidualAboveBaselineBytes != int64(10<<20) {
		t.Fatalf("noSpike regressed stats = %+v", noSpike)
	}
	noSpikeOK := ComputeReclaimStats(100<<20, 95<<20, 95<<20)
	if noSpikeOK.RatioPct != 100.0 {
		t.Fatalf("noSpikeOK stats = %+v", noSpikeOK)
	}
	negReclaim := ComputeReclaimStats(80<<20, 120<<20, 130<<20)
	if negReclaim.RatioPct != 0.0 || negReclaim.ReclaimedBytes != -int64(10<<20) {
		t.Fatalf("negReclaim stats = %+v", negReclaim)
	}

	slope := ComputeRoundSlope([]uint64{100 << 20, 102 << 20, 104 << 20, 106 << 20})
	if math.Abs(slope-float64(2<<20)) > 1.0 {
		t.Fatalf("ComputeRoundSlope = %v, want %v", slope, float64(2<<20))
	}

	rep := &report.Report{
		Scenarios: []report.ScenarioResult{
			{
				ID:      ScenarioBgIdleNormal,
				Dataset: "standard",
				Status:  "PASS",
				Phases: []sampler.PhaseSummary{
					{SteadyMedianBytes: 95 << 20, CPUPercentMean: 1.5},
				},
			},
			{
				ID:      ScenarioBgIdleLightweight,
				Dataset: "standard",
				Status:  "PASS",
				Phases: []sampler.PhaseSummary{
					{SteadyMedianBytes: 40 << 20, CPUPercentMean: 0.4},
				},
			},
		},
	}
	computeReportDerivedMetrics(rep)
	if math.Abs(rep.DerivedMetrics["tray_plus_standby_panel_primary_bytes"]-float64(55<<20)) > 1.0 {
		t.Fatalf("unexpected net delta: %+v", rep.DerivedMetrics)
	}
}

func TestParseRuntimeMetricsDirAndEvents(t *testing.T) {
	dir := t.TempDir()
	runtimeJSONL := `{"utc_millis":1000,"elapsed_sec":0,"role":"gui","pid":123,"heap_alloc":10485760,"heap_inuse":12582912,"heap_sys":20971520,"heap_released":4194304,"stack_inuse":1048576,"num_gc":3,"goroutines":20,"cache":{"entries":50,"metadata_items":200,"details":15,"image_owners":6,"selected_originals":1}}
{"utc_millis":6000,"elapsed_sec":5,"role":"gui","pid":123,"heap_alloc":15728640,"heap_inuse":16777216,"heap_sys":20971520,"heap_released":2097152,"stack_inuse":1048576,"num_gc":5,"goroutines":24,"cache":{"entries":50,"metadata_items":200,"details":18,"image_owners":8,"selected_originals":1}}
`
	if err := os.WriteFile(filepath.Join(dir, "gui-123.runtime.jsonl"), []byte(runtimeJSONL), 0o600); err != nil {
		t.Fatal(err)
	}
	sum, err := ParseRuntimeMetricsDir(dir)
	if err != nil {
		t.Fatalf("ParseRuntimeMetricsDir: %v", err)
	}
	g, ok := sum["gui"]
	if !ok || g.SampleCount != 2 || g.NumGCDelta != 2 || g.HeapAllocMaxBytes != 15728640 || g.ImageOwnersMax != 8 {
		t.Fatalf("unexpected probe summary: %+v", g)
	}

	evJSONL := `{"event":"selected-original-ready","role":"gui","pid":123,"seq":1,"selection_seq":1,"unix_millis":5500,"width":2048,"height":1536}
{"event":"selected-original-ready","role":"gui","pid":123,"seq":2,"selection_seq":2,"unix_millis":5900,"width":2048,"height":1536}
`
	if err := os.WriteFile(filepath.Join(dir, "gui-123.events.jsonl"), []byte(evJSONL), 0o600); err != nil {
		t.Fatal(err)
	}
	evs, err := ReadOriginalReadyEvents(dir, "gui", 123, 5000)
	if err != nil || len(evs) != 2 || evs[0].Width != 2048 {
		t.Fatalf("ReadOriginalReadyEvents = %+v, err=%v", evs, err)
	}
	seq, selSeq := LatestEventCursor(evs[:1])
	if matched := FilterNewOriginalReadyEvents(evs, seq, selSeq, 5800); len(matched) != 1 || matched[0].Seq != 2 {
		t.Fatalf("FilterNewOriginalReadyEvents = %+v, want seq 2 only", matched)
	}
	if matched := FilterNewOriginalReadyEvents(evs, 2, 2, 5800); len(matched) != 0 {
		t.Fatalf("expected stale events filtered out, got %+v", matched)
	}
}

func TestDaemonConvergedHelper(t *testing.T) {
	// Still at initial high startup plateau (~62 MiB) -> wait even past minWait until drop or maxWait.
	plateauHigh := []uint64{62 << 20, 63 << 20, 62 << 20, 62 << 20, 63 << 20}
	if daemonHasConverged(plateauHigh, 45*time.Second, 42*time.Second) {
		t.Fatal("expected un-dropped 62 MiB plateau not to report converged before drop")
	}
	// Explicit >=30% drop stabilized for 5s after 25s -> early converged.
	dropped := []uint64{62 << 20, 62 << 20, 26 << 20, 26 << 20, 26 << 20, 26 << 20, 26 << 20}
	if !daemonHasConverged(dropped, 30*time.Second, 42*time.Second) {
		t.Fatal("expected dropped+stabilized daemon to converge early")
	}
	// Already low <= 35 MiB flat profile reaches minWait cleanly.
	plateauLow := []uint64{26 << 20, 26 << 20, 27 << 20, 26 << 20, 26 << 20}
	if !daemonHasConverged(plateauLow, 42*time.Second, 42*time.Second) {
		t.Fatal("expected flat low stabilized daemon to converge once minWait reached")
	}
}

func TestVerifyOwnershipGuards(t *testing.T) {
	dir := t.TempDir()
	bin := filepath.Join(dir, "unic-desktop")
	if err := os.WriteFile(bin, []byte("x"), 0o755); err != nil {
		t.Fatal(err)
	}
	prof := filepath.Join(dir, "profile")
	if err := os.MkdirAll(prof, 0o700); err != nil {
		t.Fatal(err)
	}

	if err := verifyRoleOwnership(roleProbeInfo{Role: "tray", PID: 100, Profile: prof, BinaryPath: bin}, prof, bin); err != nil {
		t.Fatalf("verifyRoleOwnership valid failed: %v", err)
	}
	if err := verifyRoleOwnership(roleProbeInfo{Role: "tray", PID: 100, Profile: "/other/profile", BinaryPath: bin}, prof, bin); err == nil {
		t.Fatal("expected mismatch error for foreign profile")
	}
	if err := verifyRoleOwnership(roleProbeInfo{Role: "tray", PID: 100, Profile: prof, BinaryPath: "/other/bin"}, prof, bin); err == nil {
		t.Fatal("expected mismatch error for foreign binary")
	}
}
