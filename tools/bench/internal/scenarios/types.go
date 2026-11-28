// Package scenarios orchestrates cross-platform Track A (production) and
// Track B (interactive + bench_probe) desktop resource benchmark scenarios.
package scenarios

import (
	"fmt"
	"strings"
	"time"

	"github.com/uniclipboard/unic/tools/bench/internal/fixture"
)

// Canonical scenario identifiers.
const (
	ScenarioBgIdleNormal      = "bg-idle-normal"
	ScenarioBgIdleLightweight = "bg-idle-lightweight"
	ScenarioFgVisibleIdle     = "fg-visible-idle"
	ScenarioFgInteractive     = "fg-interactive"
	ScenarioFgCloseReclaim    = "fg-close-reclaim"
	ScenarioQpLifecycle       = "qp-lifecycle"
)

// Track identifies whether a run exercises production binaries or E2E/probe binaries.
type Track string

const (
	TrackProduction  Track = "production"
	TrackInteractive Track = "interactive"
	TrackBoth        Track = "both"
)

// Profile controls steady-state sampling durations and cycle counts (D3=A).
type Profile string

const (
	ProfileDefault Profile = "default"
	ProfileSoak    Profile = "soak"
)

// Timings holds per-profile phase durations and cycle counts.
type Timings struct {
	BgWarmup            time.Duration
	BgSteady            time.Duration
	FgWarmup            time.Duration
	FgSteady            time.Duration
	FgInteractiveSettle time.Duration
	FgCloseHold         time.Duration
	FgCloseRest         time.Duration
	FgCloseRounds       int
	FgHideHold          time.Duration
	QpColdHold          time.Duration
	QpColdRest          time.Duration
	QpWarmRounds        int
	QpWarmHold          time.Duration
	QpWarmRest          time.Duration
	SampleInterval      time.Duration
}

// TimingsForProfile returns the Gate 1 D3=A timing specification for profile.
func TimingsForProfile(p Profile) Timings {
	if p == ProfileSoak {
		return Timings{
			BgWarmup:            45 * time.Second,
			BgSteady:            300 * time.Second,
			FgWarmup:            10 * time.Second,
			FgSteady:            30 * time.Second,
			FgInteractiveSettle: 15 * time.Second,
			FgCloseHold:         10 * time.Second,
			FgCloseRest:         15 * time.Second,
			FgCloseRounds:       5,
			FgHideHold:          20 * time.Second,
			QpColdHold:          15 * time.Second,
			QpColdRest:          15 * time.Second,
			QpWarmRounds:        20,
			QpWarmHold:          5 * time.Second,
			QpWarmRest:          10 * time.Second,
			SampleInterval:      time.Second,
		}
	}
	return Timings{
		BgWarmup:            45 * time.Second,
		BgSteady:            60 * time.Second,
		FgWarmup:            10 * time.Second,
		FgSteady:            30 * time.Second,
		FgInteractiveSettle: 15 * time.Second,
		FgCloseHold:         10 * time.Second,
		FgCloseRest:         15 * time.Second,
		FgCloseRounds:       5,
		FgHideHold:          20 * time.Second,
		QpColdHold:          15 * time.Second,
		QpColdRest:          15 * time.Second,
		QpWarmRounds:        10,
		QpWarmHold:          5 * time.Second,
		QpWarmRest:          10 * time.Second,
		SampleInterval:      time.Second,
	}
}

// AllScenarioIDs returns the 6 canonical scenarios in execution order.
func AllScenarioIDs() []string {
	return []string{
		ScenarioBgIdleNormal,
		ScenarioBgIdleLightweight,
		ScenarioFgVisibleIdle,
		ScenarioFgInteractive,
		ScenarioFgCloseReclaim,
		ScenarioQpLifecycle,
	}
}

// ParseScenarios parses "all" or a comma-separated list of canonical scenario IDs.
func ParseScenarios(raw string) ([]string, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" || strings.EqualFold(trimmed, "all") {
		return AllScenarioIDs(), nil
	}
	valid := map[string]bool{}
	for _, id := range AllScenarioIDs() {
		valid[id] = true
	}
	seen := map[string]bool{}
	var out []string
	for _, part := range strings.Split(trimmed, ",") {
		id := strings.ToLower(strings.TrimSpace(part))
		if id == "" {
			continue
		}
		if !valid[id] {
			return nil, fmt.Errorf("unknown scenario %q (valid: %s)", id, strings.Join(AllScenarioIDs(), ", "))
		}
		if !seen[id] {
			seen[id] = true
			out = append(out, id)
		}
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("no valid scenarios specified")
	}
	return out, nil
}

// ParseTrack validates --track.
func ParseTrack(raw string) (Track, error) {
	switch Track(strings.ToLower(strings.TrimSpace(raw))) {
	case TrackProduction:
		return TrackProduction, nil
	case TrackInteractive:
		return TrackInteractive, nil
	case TrackBoth, "":
		return TrackBoth, nil
	default:
		return "", fmt.Errorf("invalid track %q (want production, interactive, or both)", raw)
	}
}

// ParseProfile validates --profile.
func ParseProfile(raw string) (Profile, error) {
	switch Profile(strings.ToLower(strings.TrimSpace(raw))) {
	case ProfileDefault, "":
		return ProfileDefault, nil
	case ProfileSoak:
		return ProfileSoak, nil
	default:
		return "", fmt.Errorf("invalid profile %q (want default or soak)", raw)
	}
}

// ParseDatasets parses a comma-separated list of presets (e.g. "empty,standard").
func ParseDatasets(raw string, profile Profile) ([]fixture.Preset, error) {
	trimmed := strings.TrimSpace(raw)
	if trimmed == "" {
		if profile == ProfileSoak {
			return []fixture.Preset{fixture.PresetSoak}, nil
		}
		return []fixture.Preset{fixture.PresetEmpty, fixture.PresetStandard}, nil
	}
	seen := map[fixture.Preset]bool{}
	var out []fixture.Preset
	for _, part := range strings.Split(trimmed, ",") {
		p, err := fixture.ParsePreset(part)
		if err != nil {
			return nil, err
		}
		if !seen[p] {
			seen[p] = true
			out = append(out, p)
		}
	}
	if len(out) == 0 {
		return nil, fmt.Errorf("at least one dataset is required")
	}
	return out, nil
}

// Binaries holds paths to Track A (production) and Track B (desktop_e2e+bench_probe) binaries.
type Binaries struct {
	Unic            string
	Unicd           string
	Desktop         string
	UnicdProbe      string
	DesktopE2EProbe string
}

// Options configures a complete benchmark run.
type Options struct {
	Suite       []string
	Track       Track
	Datasets    []fixture.Preset
	Profile     Profile
	Repetitions int
	OutputDir   string
	RepoRoot    string
	Binaries    Binaries
	KeepProfile bool
	Logf        func(string, ...any)
}
