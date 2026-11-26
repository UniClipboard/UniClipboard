package main

import (
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
  --name NAME     Set this device's display name in config.toml
  --fresh         Start a fresh empty profile even if UniClipboard v1 data is found
  --json          Output result as JSON
  -h, --help      Show this help

Examples:
  unic init
  unic --profile work init --name "Work MacBook"
  unic init --fresh
`

func runInit(args []string, out, errOut io.Writer, defaultJSON bool) int {
	if wantsHelp(args) {
		_, _ = fmt.Fprint(out, initUsage)
		return 0
	}
	fs := flag.NewFlagSet("unic init", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	deviceName := fs.String("name", "", "device display name")
	fresh := fs.Bool("fresh", false, "start fresh even if v1 data exists")
	jsonMode := fs.Bool("json", defaultJSON, "output JSON")
	pos, err := parse(fs, args)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			_, _ = fmt.Fprint(out, initUsage)
			return 0
		}
		_, _ = fmt.Fprintln(errOut, errorLine(err.Error()))
		_, _ = fmt.Fprint(errOut, "\n"+initUsage)
		return 2
	}
	if len(pos) != 0 {
		_, _ = fmt.Fprintln(errOut, errorLine("unexpected arguments for `unic init`"))
		_, _ = fmt.Fprint(errOut, "\n"+initUsage)
		return 2
	}

	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()

	target, err := paths.ResolveDir()
	if err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", err.Error(), "")
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
	if foundSource != "" && !*fresh {
		msg := fmt.Sprintf("found existing UniClipboard v1 data at %s\n  To import your v1 history:  unic migrate\n  To start fresh without v1:  unic init --fresh", foundSource)
		initFailure(*jsonMode, errOut, "source_present", msg, "unic migrate")
		return 1
	}

	if err := os.MkdirAll(target, 0o700); err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", err.Error(), "")
		return 1
	}
	target, err = filepath.EvalSymlinks(target)
	if err != nil {
		initFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve profile directory", "")
		return 1
	}
	unlock, err := servicelaunch.LockContext(ctx, target)
	if err != nil {
		initFailure(*jsonMode, errOut, "lock_failed", err.Error(), "")
		return 1
	}
	defer unlock()
	if ctx.Err() != nil {
		return 1
	}

	state, err := climigration.Read(target)
	if err != nil {
		initFailure(*jsonMode, errOut, "state_invalid", err.Error(), "unic migrate --help")
		return 1
	}
	if state.Status == "pending" {
		initFailure(*jsonMode, errOut, "migration_pending", "a v1 history import is in progress; finish it with `unic migrate` before using this profile", "unic migrate")
		return 1
	}

	profileName, _ := paths.ActiveProfile()
	cfgPath := filepath.Join(target, "config.toml")
	already := state.Status == "complete"

	if !already {
		// Re-check v1 sources under lock unless --fresh was specified.
		foundSource = ""
		for _, candidate := range sources {
			if !sourceAbsentOrEmpty(candidate) {
				foundSource = candidate
				break
			}
		}
		if foundSource != "" && !*fresh {
			msg := fmt.Sprintf("found existing UniClipboard v1 data at %s\n  To import your v1 history:  unic migrate\n  To start fresh without v1:  unic init --fresh", foundSource)
			initFailure(*jsonMode, errOut, "source_present", msg, "unic migrate")
			return 1
		}
		probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		if err != nil {
			var ee *engine.Error
			if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
				initFailure(*jsonMode, errOut, "in_use", "stop the running service for this profile (`unic daemon stop`) and retry `unic init`", "unic daemon stop")
			} else {
				initFailure(*jsonMode, errOut, "target_unavailable", "cannot initialize profile storage: "+err.Error(), "")
			}
			return 1
		}
		if err := probe.Close(context.Background()); err != nil {
			initFailure(*jsonMode, errOut, "target_unavailable", err.Error(), "")
			return 1
		}
		if ctx.Err() != nil {
			return 1
		}
		if _, err := os.Stat(cfgPath); errors.Is(err, os.ErrNotExist) {
			if err := config.WriteTemplate(cfgPath); err != nil {
				initFailure(*jsonMode, errOut, "config_unavailable", "cannot write default config.toml: "+err.Error(), "")
				return 1
			}
		}
		outcome := "no_source"
		if foundSource != "" && *fresh {
			outcome = "fresh"
		}
		if err := climigration.Write(target, climigration.State{Status: "complete", Outcome: outcome}); err != nil {
			initFailure(*jsonMode, errOut, "state_unavailable", err.Error(), "")
			return 1
		}
		state = climigration.State{Status: "complete", Outcome: outcome}
	}

	if strings.TrimSpace(*deviceName) != "" {
		if err := setInitialDeviceName(cfgPath, strings.TrimSpace(*deviceName)); err != nil {
			initFailure(*jsonMode, errOut, "config_unavailable", "initialized profile, but could not save --name: "+err.Error(), "")
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
	if already {
		_, _ = fmt.Fprintf(out, "%s Profile %q is already initialized\n", style(outTTY, sOK, "✓"), profileName)
	} else {
		_, _ = fmt.Fprintf(out, "%s Initialized profile %q\n", style(outTTY, sOK, "✓"), profileName)
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
