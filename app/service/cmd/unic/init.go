package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/signal"
	"path/filepath"
	"strings"

	"github.com/BurntSushi/toml"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/climigration"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/v1migration"
)

const initUsage = `Initialize a unic profile for first use.

Usage:
  unic init [flags]
  unic --profile <name> init [flags]

Flags:
  --name NAME          Set this device's display name in config.toml
  --fresh              Start a fresh empty profile even if UniClipboard v1 data is found
  --discard-pending    With --fresh, abandon a pending/partial v1 import and unlock the profile
  --json               Output result as JSON
  -h, --help           Show this help

Examples:
  unic init
  unic --profile work init --name "Work MacBook"
  unic init --fresh
  unic init --fresh --discard-pending
`

func runInit(args []string, in *os.File, out, errOut io.Writer, defaultJSON bool) int {
	if wantsHelp(args) {
		_, _ = fmt.Fprint(out, initUsage)
		return 0
	}
	fs := flag.NewFlagSet("unic init", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	deviceName := fs.String("name", "", "device display name")
	fresh := fs.Bool("fresh", false, "start fresh even if v1 data exists")
	discardPending := fs.Bool("discard-pending", false, "abandon an incomplete pending migration when used with --fresh")
	jsonMode := fs.Bool("json", defaultJSON, "output JSON")
	pos, err := parse(fs, args)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			_, _ = fmt.Fprint(out, initUsage)
			return 0
		}
		msg := sanitizeFlagParseError("unic init", err)
		if jsonRequested(args) {
			initFailure(true, errOut, "invalid_argument", msg, "")
		} else {
			_, _ = fmt.Fprintln(errOut, errorLine(msg))
			_, _ = fmt.Fprint(errOut, "\n"+initUsage)
		}
		return 2
	}
	if len(pos) != 0 {
		msg := "unexpected arguments for `unic init`"
		if *jsonMode {
			initFailure(true, errOut, "invalid_argument", msg, "")
		} else {
			_, _ = fmt.Fprintln(errOut, errorLine(msg))
			_, _ = fmt.Fprint(errOut, "\n"+initUsage)
		}
		return 2
	}
	if *discardPending && !*fresh {
		msg := "--discard-pending requires --fresh"
		if *jsonMode {
			initFailure(true, errOut, "invalid_argument", msg, "unic init --fresh --discard-pending")
		} else {
			_, _ = fmt.Fprintln(errOut, errorLine(msg))
			_, _ = fmt.Fprint(errOut, "\n"+initUsage)
		}
		return 2
	}

	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()

	target, err := paths.ResolveDir()
	if err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve profile directory", "")
		return 1
	}
	target, err = filepath.Abs(target)
	if err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve profile directory", "")
		return 1
	}

	// Check v1 source presence before mutating target.
	sources, v1Err := discoverV1Sources()
	if v1Err != nil && !*fresh {
		initFailure(*jsonMode, errOut, "source_unknown", "cannot check for UniClipboard v1 data; use `unic migrate --source DIR` or `unic init --fresh`", "unic init --fresh")
		return 1
	}
	var foundSource string
	for _, candidate := range sources {
		if err := v1migration.ValidateDirectories(candidate, target); err != nil {
			initFailure(*jsonMode, errOut, "source_target_overlap", "profile directory overlaps with UniClipboard v1 data directory", "")
			return 1
		}
		if !sourceAbsentOrEmpty(candidate) && foundSource == "" {
			foundSource = candidate
		}
	}

	prevState, prevErr := climigration.Read(target)
	if prevErr != nil {
		initFailure(*jsonMode, errOut, "state_invalid", prevErr.Error(), "unic migrate")
		return 1
	}
	if prevState.Status == "pending" && (!*fresh || !*discardPending) {
		initFailure(*jsonMode, errOut, "migration_pending", "a v1 history import is in progress; finish it with `unic migrate` or abandon it with `unic init --fresh --discard-pending`", "unic migrate")
		return 1
	}

	chosenFresh := false
	if prevState.Status == "" && foundSource != "" && !*fresh {
		if !*jsonMode && isTerminalFile(in) {
			choice, ok := promptInitV1Choice(ctx, in, errOut, foundSource)
			if !ok {
				return 1
			}
			if choice == "migrate" {
				svc := "UniClipboard"
				if defaultSrc, derr := v1migration.DefaultSource(); derr == nil && foundSource != defaultSrc {
					if profile := os.Getenv("UC_PROFILE"); profile != "" && !strings.ContainsAny(profile, `/\`) {
						svc = "UniClipboard-" + profile
					}
				}
				code := executeMigration(ctx, migrateOptions{
					source:  foundSource,
					target:  target,
					service: svc,
					in:      in,
					out:     out,
					errOut:  errOut,
				})
				if code != 0 {
					return code
				}
				cfgPath := filepath.Join(target, "config.toml")
				if _, serr := os.Stat(cfgPath); errors.Is(serr, os.ErrNotExist) {
					if werr := config.WriteTemplate(cfgPath); werr != nil {
						initFailure(false, errOut, "config_unavailable", "cannot write default config.toml", "")
						return 1
					}
				}
				if strings.TrimSpace(*deviceName) != "" {
					if werr := setInitialDeviceName(cfgPath, strings.TrimSpace(*deviceName)); werr != nil {
						initFailure(false, errOut, "config_unavailable", "migrated profile, but could not save --name (check config.toml permissions and syntax)", "")
						return 1
					}
				}
				return 0
			}
			chosenFresh = true
		} else {
			initFailure(*jsonMode, errOut, "source_present", sourcePresentMessage(*jsonMode, foundSource), "unic migrate")
			return 1
		}
	}

	if err := os.MkdirAll(target, 0o700); err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", "cannot create profile directory", "")
		return 1
	}
	target, err = filepath.EvalSymlinks(target)
	if err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve profile directory", "")
		return 1
	}
	unlock, err := servicelaunch.LockContext(ctx, target)
	if err != nil {
		initFailure(*jsonMode, errOut, "lock_failed", "cannot acquire profile lock", "")
		return 1
	}
	defer unlock()
	if ctx.Err() != nil {
		return 1
	}

	state, err := climigration.Read(target)
	if err != nil {
		initFailure(*jsonMode, errOut, "state_invalid", err.Error(), "unic migrate")
		return 1
	}
	if state.Status == "pending" && (!*fresh || !*discardPending) {
		initFailure(*jsonMode, errOut, "migration_pending", "a v1 history import is in progress; finish it with `unic migrate` or abandon it with `unic init --fresh --discard-pending`", "unic migrate")
		return 1
	}

	profileName, _ := paths.ActiveProfile()
	cfgPath := filepath.Join(target, "config.toml")
	already := state.Status == "complete"

	if !already {
		// Re-check v1 sources under lock unless --fresh or interactive choice 2 was selected.
		foundSource = ""
		for _, candidate := range sources {
			if !sourceAbsentOrEmpty(candidate) {
				foundSource = candidate
				break
			}
		}
		if state.Status != "pending" && foundSource != "" && !*fresh && !chosenFresh {
			initFailure(*jsonMode, errOut, "source_present", sourcePresentMessage(*jsonMode, foundSource), "unic migrate")
			return 1
		}
		probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		if err != nil {
			initProbeFailure(*jsonMode, errOut, err)
			return 1
		}
		if state.Status == "pending" && !*jsonMode && isTerminalFile(in) {
			// Release pre-check probe while waiting for user input so we don't hold
			// an open DB lock across human think-time, then re-acquire exclusive
			// ownership after confirmation and hold it across climigration.Write.
			if err := probe.Close(context.Background()); err != nil {
				initFailure(*jsonMode, errOut, "target_unavailable", "cannot close profile storage probe", "")
				return 1
			}
			_, _ = fmt.Fprint(errOut, "Discard pending v1 migration and initialize a fresh profile? Existing imported data will not be deleted. [y/N] ")
			line, rerr := readLineContext(ctx, in)
			if rerr != nil {
				_, _ = fmt.Fprintln(errOut, "\ncancelled")
				return 1
			}
			ans := strings.ToLower(strings.TrimSpace(line))
			if ans != "y" && ans != "yes" {
				_, _ = fmt.Fprintln(errOut, "cancelled")
				return 1
			}
			probe, err = engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
			if err != nil {
				initProbeFailure(*jsonMode, errOut, err)
				return 1
			}
		}
		if ctx.Err() != nil {
			_ = probe.Close(context.Background())
			return 1
		}
		if _, err := os.Stat(cfgPath); errors.Is(err, os.ErrNotExist) {
			if err := config.WriteTemplate(cfgPath); err != nil {
				_ = probe.Close(context.Background())
				initFailure(*jsonMode, errOut, "config_unavailable", "cannot write default config.toml", "")
				return 1
			}
		}
		outcome := "no_source"
		if (foundSource != "" && (*fresh || chosenFresh)) || (state.Status == "pending" && *fresh && *discardPending) {
			outcome = "fresh"
		}
		if err := climigration.Write(target, climigration.State{Status: "complete", Outcome: outcome}); err != nil {
			_ = probe.Close(context.Background())
			initFailure(*jsonMode, errOut, "state_unavailable", "cannot publish CLI initialization state", "")
			return 1
		}
		if err := probe.Close(context.Background()); err != nil {
			initFailure(*jsonMode, errOut, "target_unavailable", "cannot close profile storage probe", "")
			return 1
		}
		state = climigration.State{Status: "complete", Outcome: outcome}
	}

	if strings.TrimSpace(*deviceName) != "" {
		if err := setInitialDeviceName(cfgPath, strings.TrimSpace(*deviceName)); err != nil {
			initFailure(*jsonMode, errOut, "config_unavailable", "initialized profile, but could not save --name (check config.toml permissions and syntax)", "")
			return 1
		}
	}

	if *jsonMode {
		_ = json.NewEncoder(out).Encode(map[string]any{
			"status":              "complete",
			"outcome":             state.Outcome,
			"profile":             profileName,
			"directory":           target,
			"config":              cfgPath,
			"already_initialized": already,
		})
		return 0
	}

	pFlag := ""
	if p := paths.ExplicitProfile(); p != "" && p != paths.DefaultProfileName {
		pFlag = " -p " + p
	}
	tty := isTerminalFile(out)
	if already {
		_, _ = fmt.Fprintf(out, "%s Profile %q is already initialized\n", style(tty, sOK, "✓"), profileName)
	} else {
		_, _ = fmt.Fprintf(out, "%s Initialized profile %q\n", style(tty, sOK, "✓"), profileName)
	}
	_, _ = fmt.Fprintf(out, "  directory  %s\n", target)
	_, _ = fmt.Fprintf(out, "  config     %s\n", cfgPath)
	if !already {
		_, _ = fmt.Fprintf(out, "\nNext steps:\n")
		_, _ = fmt.Fprintf(out, "  unic%s daemon start       Keep syncing in the background\n", pFlag)
		_, _ = fmt.Fprintf(out, "  unic%s send \"hello\"       Share once with a 6-digit code\n", pFlag)
		_, _ = fmt.Fprintf(out, "  unic%s device add         Pair another device\n", pFlag)
	}
	return 0
}

func promptInitV1Choice(ctx context.Context, in *os.File, errOut io.Writer, foundSource string) (string, bool) {
	_, _ = fmt.Fprintf(errOut, "Found existing UniClipboard v1 data at %s\n", foundSource)
	_, _ = fmt.Fprintln(errOut, "  [1] Import v1 history (recommended)")
	_, _ = fmt.Fprintln(errOut, "  [2] Start fresh without v1 history")
	for attempt := 0; attempt < 3; attempt++ {
		_, _ = fmt.Fprint(errOut, "Select [1/2] (default 1, q to cancel): ")
		line, err := readLineContext(ctx, in)
		if err != nil {
			_, _ = fmt.Fprintln(errOut, "\ncancelled")
			return "", false
		}
		switch strings.ToLower(strings.TrimSpace(line)) {
		case "", "1":
			return "migrate", true
		case "2":
			return "fresh", true
		case "q", "quit", "c", "cancel":
			_, _ = fmt.Fprintln(errOut, "cancelled")
			return "", false
		default:
			_, _ = fmt.Fprintln(errOut, "Please enter 1, 2, or q.")
		}
	}
	_, _ = fmt.Fprintln(errOut, "cancelled")
	return "", false
}

func readLineContext(ctx context.Context, in *os.File) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	type result struct {
		line string
		err  error
	}
	done := make(chan result, 1)
	go func() {
		line, err := bufio.NewReader(io.LimitReader(in, 256)).ReadString('\n')
		if err == io.EOF && len(line) > 0 {
			err = nil
		}
		done <- result{line: line, err: err}
	}()
	select {
	case r := <-done:
		return r.line, r.err
	case <-ctx.Done():
		return "", ctx.Err()
	}
}

func discoverV1Sources() ([]string, error) {
	source, err := v1migration.DefaultSource()
	if err != nil {
		return nil, err
	}
	sources := []string{source}
	if profile := os.Getenv("UC_PROFILE"); profile != "" && !strings.ContainsAny(profile, `/\`) {
		sources = append(sources, source+"-"+profile)
	}
	return sources, nil
}

func setInitialDeviceName(cfgPath, name string) error {
	if _, err := os.Stat(cfgPath); errors.Is(err, os.ErrNotExist) {
		if err := config.WriteTemplate(cfgPath); err != nil {
			return err
		}
	}
	m, err := config.Open(cfgPath, func(string, ...any) {})
	if err != nil {
		return err
	}
	cfg := m.Current()
	cfg.DeviceName = name
	var buf bytes.Buffer
	if err := toml.NewEncoder(&buf).Encode(cfg); err != nil {
		return err
	}
	_, err = m.Replace(m.State().Revision, buf.Bytes())
	return err
}

func initProbeFailure(jsonMode bool, errOut io.Writer, err error) {
	var ee *engine.Error
	if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
		initFailure(jsonMode, errOut, "in_use", "stop the running service for this profile (`unic daemon stop`) and retry `unic init`", "unic daemon stop")
		return
	}
	initFailure(jsonMode, errOut, "target_unavailable", "cannot initialize profile storage", "")
}

func sourcePresentMessage(jsonMode bool, foundSource string) string {
	if jsonMode {
		return "found existing UniClipboard v1 data; run 'unic migrate' to import your v1 history or 'unic init --fresh' to start fresh without v1"
	}
	return fmt.Sprintf("found existing UniClipboard v1 data at %s\n  To import your v1 history:  unic migrate\n  To start fresh without v1:  unic init --fresh", foundSource)
}

func initFailure(jsonMode bool, w io.Writer, reason, message, next string) {
	if jsonMode {
		payload := map[string]string{"error": message, "reason": reason}
		if next != "" {
			payload["next_command"] = next
		}
		_ = json.NewEncoder(w).Encode(payload)
		return
	}
	_, _ = fmt.Fprintln(w, errorLine(message))
}
