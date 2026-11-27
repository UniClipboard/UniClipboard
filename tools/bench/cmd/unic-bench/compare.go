package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/uniclipboard/unic/tools/bench/internal/report"
)

func cmdCompare(args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("compare", flag.ContinueOnError)
	fs.SetOutput(stderr)
	budgetPath := fs.String("budget", "", "optional absolute budget JSON file")
	failOnReg := fs.Bool("fail-on-regression", false, "exit non-zero when any regression or budget alert occurs")
	outPath := fs.String("out", "", "optional path to write comparison output")
	asJSON := fs.Bool("json", false, "output JSON instead of Markdown")
	memPct := fs.Float64("mem-threshold-pct", report.DefaultThresholds().MemoryRelPct, "relative memory regression threshold (%)")
	cpuPct := fs.Float64("cpu-threshold-pct", report.DefaultThresholds().CPURelPct, "relative CPU regression threshold (%)")
	latMs := fs.Float64("latency-threshold-ms", report.DefaultThresholds().LatencyDeltaMs, "latency regression delta threshold (ms)")
	leakMiB := fs.Float64("leak-threshold-mib", 1.0, "leak slope threshold (MiB/round)")

	// Support flags either before or after positional <base.json> <cand.json>.
	pos, err := parseFlagsInterspersed(fs, args)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return &exitError{code: exitUsage, err: err}
	}
	if len(pos) != 2 {
		return &exitError{code: exitUsage, err: errors.New("usage: unic-bench compare <base.json> <cand.json> [--budget <file>] [--fail-on-regression]")}
	}

	base, err := report.ReadJSON(pos[0])
	if err != nil {
		return err
	}
	cand, err := report.ReadJSON(pos[1])
	if err != nil {
		return err
	}
	var budget *report.BudgetFile
	if *budgetPath != "" {
		budget, err = report.LoadBudget(*budgetPath)
		if err != nil {
			return err
		}
	}

	res, err := report.Compare(base, cand, report.CompareOptions{
		Thresholds: report.Thresholds{
			MemoryRelPct:           *memPct,
			CPURelPct:              *cpuPct,
			LatencyDeltaMs:         *latMs,
			LeakSlopeBytesPerRound: *leakMiB * 1024 * 1024,
		},
		Budget: budget,
	})
	if err != nil {
		return err
	}

	var formatted string
	if *asJSON {
		raw, err := json.MarshalIndent(res, "", "  ")
		if err != nil {
			return err
		}
		formatted = string(raw) + "\n"
	} else {
		formatted = report.RenderCompareMarkdown(res)
	}

	if _, err := io.WriteString(stdout, formatted); err != nil {
		return err
	}
	if *outPath != "" {
		if err := os.WriteFile(*outPath, []byte(formatted), 0o600); err != nil {
			return fmt.Errorf("write comparison output: %w", err)
		}
	}

	if *failOnReg && res.HasRegressions() {
		return &exitError{
			code: exitRegression,
			err:  fmt.Errorf("%d regression/budget finding(s) detected", len(res.Findings)),
		}
	}
	return nil
}

func parseFlagsInterspersed(fs *flag.FlagSet, args []string) ([]string, error) {
	var pos []string
	for {
		if err := fs.Parse(args); err != nil {
			return nil, err
		}
		if fs.NArg() == 0 {
			return pos, nil
		}
		pos = append(pos, fs.Arg(0))
		args = fs.Args()[1:]
	}
}
