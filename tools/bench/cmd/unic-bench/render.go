package main

import (
	"errors"
	"flag"
	"fmt"
	"io"
	"strings"

	"github.com/uniclipboard/unic/tools/bench/internal/report"
)

type stringSliceFlag []string

func (s *stringSliceFlag) String() string {
	return strings.Join(*s, ",")
}

func (s *stringSliceFlag) Set(val string) error {
	trimmed := strings.TrimSpace(val)
	if trimmed == "" {
		return errors.New("value cannot be empty")
	}
	*s = append(*s, trimmed)
	return nil
}

func cmdRender(args []string, stdout, stderr io.Writer) error {
	fs := flag.NewFlagSet("render", flag.ContinueOnError)
	fs.SetOutput(stderr)

	var inFlags stringSliceFlag
	var labelFlags stringSliceFlag
	fs.Var(&inFlags, "in", "input report.json path (repeatable; positional args also accepted)")
	fs.Var(&labelFlags, "label", "optional display label matching each input report (repeatable)")
	outPath := fs.String("out", "", "output HTML file path (empty or '-' writes HTML to stdout)")
	title := fs.String("title", "", "optional HTML page title")
	doCompare := fs.Bool("compare", false, "include regression comparison between two reports (requires exactly 2 inputs)")
	budgetPath := fs.String("budget", "", "optional absolute budget JSON file (used with --compare or single-report check)")
	memPct := fs.Float64("mem-threshold-pct", report.DefaultThresholds().MemoryRelPct, "relative memory regression threshold (%)")
	cpuPct := fs.Float64("cpu-threshold-pct", report.DefaultThresholds().CPURelPct, "relative CPU regression threshold (%)")
	latMs := fs.Float64("latency-threshold-ms", report.DefaultThresholds().LatencyDeltaMs, "latency regression delta threshold (ms)")
	leakMiB := fs.Float64("leak-threshold-mib", 1.0, "leak slope threshold (MiB/round)")

	pos, err := parseFlagsInterspersed(fs, args)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return &exitError{code: exitUsage, err: err}
	}

	inputs := make([]string, 0, len(inFlags)+len(pos))
	inputs = append(inputs, inFlags...)
	inputs = append(inputs, pos...)
	if len(inputs) == 0 {
		return &exitError{code: exitUsage, err: errors.New("usage: unic-bench render [--in <report.json>]... [--out <file.html>] [--compare] [--budget <file>]")}
	}
	if len(labelFlags) > len(inputs) {
		return &exitError{code: exitUsage, err: fmt.Errorf("got %d --label flags for %d input report(s)", len(labelFlags), len(inputs))}
	}
	if *doCompare && len(inputs) != 2 {
		return &exitError{code: exitUsage, err: fmt.Errorf("--compare requires exactly 2 input reports, got %d", len(inputs))}
	}

	reports := make([]*report.Report, 0, len(inputs))
	for _, p := range inputs {
		r, rerr := report.ReadJSON(p)
		if rerr != nil {
			return rerr
		}
		reports = append(reports, r)
	}

	var budget *report.BudgetFile
	if *budgetPath != "" {
		budget, err = report.LoadBudget(*budgetPath)
		if err != nil {
			return err
		}
	}

	var cmpRes *report.CompareResult
	if *doCompare || budget != nil {
		base, cand := reports[0], reports[0]
		if len(reports) >= 2 {
			if !*doCompare && len(reports) > 2 {
				return &exitError{code: exitUsage, err: errors.New("--budget with multiple inputs requires --compare with exactly 2 reports")}
			}
			cand = reports[1]
		}
		cmpRes, err = report.Compare(base, cand, report.CompareOptions{
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
	}

	htmlOpts := report.HTMLOptions{
		Title:      *title,
		Reports:    reports,
		Labels:     []string(labelFlags),
		Comparison: cmpRes,
		Budget:     budget,
	}

	targetOut := strings.TrimSpace(*outPath)
	if targetOut == "" || targetOut == "-" {
		rendered, rerr := report.RenderHTML(htmlOpts)
		if rerr != nil {
			return rerr
		}
		_, err = io.WriteString(stdout, rendered)
		return err
	}

	if err := report.WriteHTML(targetOut, htmlOpts); err != nil {
		return err
	}

	_, _ = fmt.Fprintf(stdout, "Wrote HTML benchmark report to %s (reports=%d, compare=%t)\n", targetOut, len(reports), cmpRes != nil)
	return nil
}
