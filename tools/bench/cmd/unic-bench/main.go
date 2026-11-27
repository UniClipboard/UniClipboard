// Command unic-bench measures cross-platform Windows and macOS desktop resource
// consumption, generates deterministic corpora, verifies benchmark environments,
// and compares reports against relative thresholds and absolute budgets.
package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"os/signal"
	"strings"
)

const usageText = `unic-bench - cross-platform Windows & macOS desktop resource benchmark

Usage:
  unic-bench <command> [flags]

Commands:
  doctor    Verify OS tools, native sampling, clipboard sequence, and binary architectures
  seed      Generate deterministic synthetic corpus and optionally seed an isolated profile
  compare   Compare candidate report.json against baseline and optional budget file
  run       Run desktop resource benchmark scenarios (Slice 2)

Run 'unic-bench <command> --help' for command-specific flags.
`

const (
	exitOK         = 0
	exitFailed     = 1
	exitUsage      = 2
	exitRegression = 3
)

type exitError struct {
	code int
	err  error
}

func (e *exitError) Error() string { return e.err.Error() }
func (e *exitError) Unwrap() error { return e.err }

func main() {
	os.Exit(runCLI(context.Background(), os.Args[1:], os.Stdout, os.Stderr))
}

func runCLI(ctx context.Context, args []string, stdout, stderr io.Writer) int {
	ctx, stop := signal.NotifyContext(ctx, os.Interrupt)
	defer stop()

	if len(args) == 0 {
		_, _ = fmt.Fprint(stderr, usageText)
		return exitUsage
	}

	cmd, rest := args[0], args[1:]
	var err error
	switch strings.ToLower(cmd) {
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(stdout, usageText)
		return exitOK
	case "doctor":
		err = cmdDoctor(ctx, rest, stdout, stderr)
	case "seed":
		err = cmdSeed(ctx, rest, stdout, stderr)
	case "compare":
		err = cmdCompare(rest, stdout, stderr)
	case "run":
		err = cmdRun(ctx, rest, stdout, stderr)
	default:
		_, _ = fmt.Fprintf(stderr, "unknown command %q\n\n%s", cmd, usageText)
		return exitUsage
	}

	if err == nil {
		return exitOK
	}
	var ee *exitError
	if errors.As(err, &ee) {
		_, _ = fmt.Fprintln(stderr, "error:", ee.err)
		return ee.code
	}
	_, _ = fmt.Fprintln(stderr, "error:", err)
	return exitFailed
}
