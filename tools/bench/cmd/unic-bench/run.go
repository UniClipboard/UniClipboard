package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"path/filepath"
	"time"

	"github.com/uniclipboard/unic/tools/bench/internal/scenarios"
)

func cmdRun(ctx context.Context, args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("run", flag.ContinueOnError)
	fs.SetOutput(stderr)
	suiteStr := fs.String("suite", "all", "comma-separated scenario IDs or 'all'")
	trackStr := fs.String("track", string(scenarios.TrackBoth), "benchmark track: production, interactive, or both")
	datasetStr := fs.String("dataset", "", "comma-separated corpus datasets: empty, standard, or soak (defaults by --profile)")
	profileStr := fs.String("profile", string(scenarios.ProfileDefault), "timing profile: default or soak")
	repetitions := fs.Int("repetitions", 3, "repeat rounds for steady-state scenarios")
	outDir := fs.String("output", filepath.Join(".build", "bench", time.Now().UTC().Format("20060102-150405")), "artifact output directory")
	unicBin := fs.String("unic", "", "optional prebuilt unic binary")
	unicdBin := fs.String("unicd", "", "optional prebuilt production unicd binary")
	desktopBin := fs.String("desktop", "", "optional prebuilt production unic-desktop binary")
	unicdProbeBin := fs.String("unicd-probe", "", "optional prebuilt bench_probe unicd binary")
	desktopE2EBin := fs.String("desktop-e2e", "", "optional prebuilt desktop_e2e,bench_probe unic-desktop binary")
	keepProfile := fs.Bool("keep-profile", false, "preserve temporary UNIC_HOME after run")
	asJSON := fs.Bool("json", false, "print report.json summary to stdout")

	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return &exitError{code: exitUsage, err: err}
	}
	if fs.NArg() != 0 {
		return &exitError{code: exitUsage, err: fmt.Errorf("unexpected positional arguments: %v", fs.Args())}
	}

	suite, err := scenarios.ParseScenarios(*suiteStr)
	if err != nil {
		return &exitError{code: exitUsage, err: err}
	}
	track, err := scenarios.ParseTrack(*trackStr)
	if err != nil {
		return &exitError{code: exitUsage, err: err}
	}
	profile, err := scenarios.ParseProfile(*profileStr)
	if err != nil {
		return &exitError{code: exitUsage, err: err}
	}
	datasets, err := scenarios.ParseDatasets(*datasetStr, profile)
	if err != nil {
		return &exitError{code: exitUsage, err: err}
	}
	if *repetitions < 1 {
		return &exitError{code: exitUsage, err: errors.New("--repetitions must be >= 1")}
	}

	opts := scenarios.Options{
		Suite:       suite,
		Track:       track,
		Datasets:    datasets,
		Profile:     profile,
		Repetitions: *repetitions,
		OutputDir:   *outDir,
		Binaries: scenarios.Binaries{
			Unic:            *unicBin,
			Unicd:           *unicdBin,
			Desktop:         *desktopBin,
			UnicdProbe:      *unicdProbeBin,
			DesktopE2EProbe: *desktopE2EBin,
		},
		KeepProfile: *keepProfile,
		Logf: func(format string, a ...any) {
			_, _ = fmt.Fprintf(stderr, "[unic-bench] "+format+"\n", a...)
		},
	}

	rep, runErr := scenarios.Run(ctx, opts)
	if rep != nil {
		if *asJSON {
			enc := json.NewEncoder(stdout)
			enc.SetIndent("", "  ")
			_ = enc.Encode(rep)
		} else {
			_, _ = fmt.Fprintf(stdout, "Wrote benchmark artifacts to %s (scenarios=%d, clipboard_unchanged=%t)\n",
				*outDir, len(rep.Scenarios), rep.ClipboardIntegrity.Unchanged)
		}
	}
	return runErr
}
