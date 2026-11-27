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
	"time"

	"golang.org/x/term"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/climigration"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/v1migration"
)

const migrateUsage = `Import UniClipboard v1 history offline into a v2 profile.

Usage:
  unic migrate [flags]

Flags:
  --source DIR         v1 data directory (default: local desktop profile)
  --target DIR         v2 data directory (default: UNIC_HOME or active profile)
  --service NAME       Old credential service (default: UniClipboard)
  --dry-run            Read and verify v1 content without writing target
  --passphrase-stdin   Read one passphrase line from stdin (never pass in args/env)
  --initialize-empty   Initialize an empty profile when no v1 source exists
  --json               Emit JSON progress and final report
  -h, --help           Show this help

Notes:
  Back up v1 data and stop v1 / unicd for the target profile before importing.
  Persisted favorites (as tag 'favorite') and unset display name, theme, analytics,
  tag layout, and shortcut preferences migrate; legacy-only favorites warn as partial.
  Device identities, pairings, and network/other settings do not migrate; pair devices again.
  Without v1 data (or to skip it), run 'unic init' or 'unic init --fresh'.
  Unrelated commands exit 6 (migration_required) until initialization completes.
  Exit codes: 0 complete/ready, 3 partial (rerun to resume), 1 failed, 2 usage.
`

type migrateOptions struct {
	source        string
	target        string
	service       string
	dryRun        bool
	jsonMode      bool
	stdinPassword bool
	in            *os.File
	out           io.Writer
	errOut        io.Writer
	secretReader  func(ctx context.Context, service, account string) ([]byte, error)
	writeState    func(dir string, state climigration.State) error
}

// runMigration imports local UniClipboard v1 history in-process, without
// starting networking or clipboard integration. It returns the exit code:
// 0 complete, 3 partial, 1 failed, 2 usage.
func runMigration(args []string, in *os.File, out, errOut io.Writer) int {
	if len(args) == 1 && (args[0] == "--help" || args[0] == "-h") {
		if _, err := fmt.Fprint(out, migrateUsage); err != nil {
			return 1
		}
		return 0
	}
	fs := flag.NewFlagSet("unic migrate", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	source := fs.String("source", "", "v1 data directory (default: local desktop profile)")
	target := fs.String("target", "", "unic data directory (default: UNIC_HOME or normal unic directory)")
	service := fs.String("service", "UniClipboard", "old credential service; named profiles use UniClipboard-PROFILE")
	initializeEmpty := fs.Bool("initialize-empty", false, "explicitly initialize when no known v1 source exists and no import is pending")
	dry := fs.Bool("dry-run", false, "read and authenticate content without changing the target")
	jsonMode := fs.Bool("json", false, "emit JSON progress and final report")
	stdinPassword := fs.Bool("passphrase-stdin", false, "read old passphrase as one line from stdin; never pass it as an argument")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			if _, werr := fmt.Fprint(out, migrateUsage); werr != nil {
				return 1
			}
			return 0
		}
		asJSON := jsonRequested(args)
		if hasFlagNamed(args, "fresh") {
			msg := "unknown flag --fresh; to start a fresh profile without v1 data, run 'unic init --fresh'"
			if asJSON {
				_ = json.NewEncoder(errOut).Encode(map[string]string{"error": msg, "reason": "invalid_argument", "next_command": "unic init --fresh"})
			} else {
				_, _ = fmt.Fprintln(errOut, errorLine(msg))
			}
			return 2
		}
		msg := sanitizeFlagParseError("unic migrate", err)
		if asJSON {
			_ = json.NewEncoder(errOut).Encode(map[string]string{"error": msg, "reason": "invalid_argument"})
		} else {
			_, _ = fmt.Fprintln(errOut, errorLine(msg))
		}
		return 2
	}
	if fs.NArg() != 0 {
		msg := "unexpected positional arguments; run 'unic migrate --help' for usage"
		if *jsonMode {
			_ = json.NewEncoder(errOut).Encode(map[string]string{"error": msg, "reason": "invalid_argument", "next_command": "unic migrate --help"})
		} else {
			_, _ = fmt.Fprintln(errOut, errorLine(msg))
		}
		return 2
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()
	if *target == "" {
		var err error
		*target, err = paths.ResolveDir()
		if err != nil {
			migrationFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve migration target", "unic migrate --help")
			return 1
		}
	}
	targetPath, err := filepath.Abs(*target)
	if err != nil {
		migrationFailure(*jsonMode, errOut, "target_unavailable", "cannot resolve migration target", "unic migrate --help")
		return 1
	}
	*target = targetPath
	if *initializeEmpty {
		incompatible := false
		fs.Visit(func(f *flag.Flag) {
			if f.Name == "source" || f.Name == "service" || f.Name == "dry-run" || f.Name == "passphrase-stdin" {
				incompatible = true
			}
		})
		if incompatible {
			msg := "--initialize-empty only accepts --target and --json"
			if *jsonMode {
				_ = json.NewEncoder(errOut).Encode(map[string]string{"error": msg, "reason": "invalid_argument"})
			} else {
				_, _ = fmt.Fprintln(errOut, errorLine(msg))
			}
			return 2
		}
		return initializeWithoutSource(ctx, *target, *jsonMode, out, errOut)
	}
	return executeMigration(ctx, migrateOptions{
		source:        *source,
		target:        *target,
		service:       *service,
		dryRun:        *dry,
		jsonMode:      *jsonMode,
		stdinPassword: *stdinPassword,
		in:            in,
		out:           out,
		errOut:        errOut,
	})
}

func executeMigration(ctx context.Context, opts migrateOptions) int {
	source := opts.source
	if source == "" {
		var err error
		source, err = v1migration.DefaultSource()
		if err != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "source_invalid", "Cannot locate UniClipboard v1 data. Run 'unic init' to initialize without v1 data, or specify '--source DIR'.", "unic init")
			return 1
		}
	}
	// Validate only public structure before checking target or requesting credentials.
	st, err := os.Stat(filepath.Join(source, "vault", "keyslot.json"))
	if err != nil || !st.Mode().IsRegular() || st.Size() > 8<<20 {
		migrationFailure(opts.jsonMode, opts.errOut, "source_invalid", "No valid UniClipboard v1 data found. Run 'unic init' to initialize without v1 data, or use 'unic migrate --source DIR' to specify a v1 directory.", "unic init")
		return 1
	}
	if err = v1migration.ValidateDirectories(source, opts.target); err != nil {
		migrationFailure(opts.jsonMode, opts.errOut, "source_target_overlap", "Source and target must be separate directories.", "unic migrate --help")
		return 1
	}
	if !opts.dryRun {
		if prevState, rerr := climigration.Read(opts.target); rerr == nil && prevState.Status == "pending" &&
			prevState.SourceID == "" && prevState.KeyslotSHA256 != "" {
			keyslotSum, kerr := climigration.KeyslotSHA256(source)
			if kerr != nil {
				migrationFailure(opts.jsonMode, opts.errOut, "source_invalid", kerr.Error(), "unic migrate --help")
				return 1
			}
			if prevState.KeyslotSHA256 != keyslotSum {
				migrationFailure(opts.jsonMode, opts.errOut, "source_mismatch", climigration.ErrSourceMismatch.Error(), "unic migrate")
				return 1
			}
		}
	}

	s, code := unlockMigrationSource(ctx, source, opts)
	if code != 0 {
		return code
	}
	defer func() { _ = s.Close() }()

	target := opts.target
	if !opts.dryRun {
		if err = os.MkdirAll(target, 0700); err != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "target_unavailable", "cannot create migration target directory", "unic migrate --help")
			return 1
		}
		target, err = filepath.EvalSymlinks(target)
		if err != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "target_unavailable", "cannot resolve migration target", "unic migrate --help")
			return 1
		}
		unlock, err := servicelaunch.LockContext(ctx, target)
		if err != nil {
			reason := "lock_failed"
			if ctx.Err() != nil {
				reason = "cancelled"
			}
			migrationFailure(opts.jsonMode, opts.errOut, reason, "cannot acquire profile transition lock", "unic migrate")
			return 1
		}
		defer unlock()
		if err = ctx.Err(); err != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "cancelled", err.Error(), "unic migrate")
			return 1
		}
		if prevState, rerr := climigration.Read(target); rerr == nil && prevState.Status == "pending" {
			if prevState.SourceID != "" && prevState.SourceID != s.ID() {
				migrationFailure(opts.jsonMode, opts.errOut, "source_mismatch", climigration.ErrSourceMismatch.Error(), "unic migrate")
				return 1
			}
			if prevState.SourceID == "" && prevState.KeyslotSHA256 != "" {
				keyslotSum, kerr := climigration.KeyslotSHA256(source)
				if kerr != nil || prevState.KeyslotSHA256 != keyslotSum {
					migrationFailure(opts.jsonMode, opts.errOut, "source_mismatch", climigration.ErrSourceMismatch.Error(), "unic migrate")
					return 1
				}
			}
		}
		probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		if err != nil {
			migrationTargetFailure(opts.jsonMode, opts.errOut, err)
			return 1
		}
		if err = probe.Close(context.Background()); err != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "target_unavailable", "cannot close migration target probe", "unic migrate --help")
			return 1
		}
		pending, err := climigration.Begin(target, source)
		if err != nil {
			reason := "state_unavailable"
			msg := "cannot update CLI migration state"
			if errors.Is(err, climigration.ErrSourceMismatch) {
				reason = "source_mismatch"
				msg = err.Error()
			}
			migrationFailure(opts.jsonMode, opts.errOut, reason, msg, "unic migrate")
			return 1
		}
		if _, err = climigration.Bind(target, pending, s.ID()); err != nil {
			reason := "state_unavailable"
			msg := "cannot update CLI migration state"
			if errors.Is(err, climigration.ErrSourceMismatch) {
				reason = "source_mismatch"
				msg = err.Error()
			}
			migrationFailure(opts.jsonMode, opts.errOut, reason, msg, "unic migrate")
			return 1
		}
	}

	var lastStage string
	errTerm := isTerminalFile(opts.errOut)
	report, err := s.Run(ctx, v1migration.RunOptions{Target: target, DryRun: opts.dryRun, Progress: func(p v1migration.Progress) {
		if p.Stage != lastStage || p.Processed == 1 || p.Processed%25 == 0 || p.Processed == p.Total {
			if opts.jsonMode {
				_ = json.NewEncoder(opts.out).Encode(p)
			} else {
				label := "Importing"
				if p.Stage == "verify" {
					label = "Verifying"
				}
				if errTerm {
					if lastStage != "" && p.Stage != lastStage {
						_, _ = fmt.Fprintln(opts.errOut)
					}
					_, _ = fmt.Fprintf(opts.errOut, "\r\x1b[2K%s %d/%d", label, p.Processed, p.Total)
				} else {
					_, _ = fmt.Fprintf(opts.errOut, "%s %d/%d\n", label, p.Processed, p.Total)
				}
			}
			lastStage = p.Stage
		}
	}})
	if !opts.jsonMode && errTerm && lastStage != "" {
		_, _ = fmt.Fprintln(opts.errOut)
	}
	emitReport := func() {
		if opts.jsonMode {
			_ = json.NewEncoder(opts.out).Encode(report)
		} else {
			printMigrationSummary(opts.out, report, opts.dryRun)
		}
	}
	if err != nil {
		emitReport()
		if opts.jsonMode {
			if ctx.Err() != nil {
				_, _ = fmt.Fprintln(opts.errOut, ctx.Err())
			} else {
				_, _ = fmt.Fprintln(opts.errOut, "v1 migration failed")
			}
		} else {
			_, _ = fmt.Fprintln(opts.errOut, err)
		}
		return 1
	}
	if report.Status == "partial" {
		emitReport()
		return 3
	}
	if opts.dryRun {
		emitReport()
		if report.Status != "ready" && report.Status != "complete" {
			return 1
		}
		return 0
	}
	if report.Status != "complete" || ctx.Err() != nil {
		if ctx.Err() != nil {
			migrationFailure(opts.jsonMode, opts.errOut, "cancelled", ctx.Err().Error(), "unic migrate")
		} else {
			migrationFailure(opts.jsonMode, opts.errOut, "migration_failed", "migration did not complete", "unic migrate")
		}
		return 1
	}
	// Publish the complete receipt under exclusive Engine ownership before
	// reporting completion to stdout; any persistence failure keeps pending state.
	if err = s.Close(); err != nil {
		migrationFailure(opts.jsonMode, opts.errOut, "source_close_failed", "cannot close source; migration remains incomplete", "unic migrate")
		return 1
	}
	probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
	if err != nil {
		migrationTargetFailure(opts.jsonMode, opts.errOut, err)
		return 1
	}
	writeState := opts.writeState
	if writeState == nil {
		writeState = climigration.Write
	}
	if err = writeState(target, climigration.State{Status: "complete", Outcome: "imported", SourceID: s.ID()}); err != nil {
		_ = probe.Close(context.Background())
		migrationFailure(opts.jsonMode, opts.errOut, "state_unavailable", "cannot publish CLI migration state", "unic migrate")
		return 1
	}
	if err = probe.Close(context.Background()); err != nil {
		migrationFailure(opts.jsonMode, opts.errOut, "target_unavailable", "cannot close migration target probe", "unic migrate --help")
		return 1
	}
	emitReport()
	return 0
}

func unlockMigrationSource(ctx context.Context, source string, opts migrateOptions) (*v1migration.Source, int) {
	secretFn := opts.secretReader
	if secretFn == nil {
		secretFn = v1migration.ReadSecretContext
	}
	var password []byte
	defer func() { clear(password) }()

	if opts.stdinPassword {
		var err error
		password, err = readMigrationPassphrase(ctx, opts.in, isTerminalFile(opts.in))
		if err != nil {
			clear(password)
			reason := "unlock_failed"
			msg := "cannot read old passphrase; migration remains incomplete"
			if ctx.Err() != nil {
				reason = "cancelled"
				msg = ctx.Err().Error()
			}
			migrationFailure(opts.jsonMode, opts.errOut, reason, msg, "unic migrate")
			return nil, 1
		}
		if len(password) == 0 {
			clear(password)
			migrationFailure(opts.jsonMode, opts.errOut, "unlock_failed", "old passphrase cannot be empty", "unic migrate")
			return nil, 1
		}
		s, err := v1migration.Open(ctx, source, opts.service, password, nil)
		clear(password)
		if err != nil {
			reason := "unlock_failed"
			if ctx.Err() != nil {
				reason = "cancelled"
			}
			migrationFailure(opts.jsonMode, opts.errOut, reason, err.Error(), "unic migrate")
			return nil, 1
		}
		return s, 0
	}

	if !opts.jsonMode {
		_, _ = fmt.Fprintln(opts.errOut, "Unlocking old history; allow the system credential prompt or enter the old passphrase if requested.")
	}
	s, err := v1migration.Open(ctx, source, opts.service, nil, func(service, account string) ([]byte, error) {
		return secretFn(ctx, service, account)
	})
	if err != nil && errors.Is(err, v1migration.ErrUnlock) {
		if !isTerminalFile(opts.in) {
			msg := fmt.Sprintf("%s; run interactively in a terminal or pass --passphrase-stdin", err.Error())
			if opts.jsonMode {
				migrationFailure(true, opts.errOut, "unlock_failed", msg, "unic migrate --passphrase-stdin")
			} else {
				_, _ = fmt.Fprintln(opts.errOut, errorLine(msg))
			}
			return nil, 1
		}
		_, _ = fmt.Fprintln(opts.errOut, err.Error())
		const maxPassphraseAttempts = 3
		for attempt := 1; attempt <= maxPassphraseAttempts; attempt++ {
			_, _ = fmt.Fprint(opts.errOut, "Old UniClipboard passphrase: ")
			password, err = readMigrationPassphrase(ctx, opts.in, true)
			_, _ = fmt.Fprintln(opts.errOut)
			if err != nil {
				clear(password)
				break
			}
			if len(password) == 0 {
				clear(password)
				_, _ = fmt.Fprintln(opts.errOut, "Passphrase cannot be empty.")
				if attempt == maxPassphraseAttempts {
					err = errors.New("v1 migration: passphrase cannot be empty; run 'unic migrate' to retry or 'unic init --fresh' to start without v1 history")
					break
				}
				_, _ = fmt.Fprintf(opts.errOut, "%d attempt(s) remaining. Press Ctrl+C to cancel (or run 'unic init --fresh' to start without v1 history).\n", maxPassphraseAttempts-attempt)
				continue
			}
			s, err = v1migration.Open(ctx, source, opts.service, password, nil)
			clear(password)
			if err == nil {
				break
			}
			if !errors.Is(err, v1migration.ErrUnlock) || ctx.Err() != nil {
				break
			}
			if attempt == maxPassphraseAttempts {
				err = errors.New("v1 migration: incorrect passphrase (3/3 attempts failed); run 'unic migrate' to retry or 'unic init --fresh' to start without v1 history")
				break
			}
			_, _ = fmt.Fprintf(opts.errOut, "Incorrect passphrase. %d attempt(s) remaining. Press Ctrl+C to cancel (or run 'unic init --fresh' to start without v1 history).\n", maxPassphraseAttempts-attempt)
		}
	}
	if err != nil {
		if opts.jsonMode {
			reason := "unlock_failed"
			if ctx.Err() != nil {
				reason = "cancelled"
			}
			migrationFailure(true, opts.errOut, reason, err.Error(), "unic init --fresh")
		} else {
			_, _ = fmt.Fprintln(opts.errOut, errorLine(err.Error()))
		}
		return nil, 1
	}
	return s, 0
}

func printMigrationSummary(out io.Writer, report v1migration.Report, dryRun bool) {
	tty := isTerminalFile(out)
	switch report.Status {
	case "complete":
		_, _ = fmt.Fprintf(out, "%s Migration complete\n", style(tty, sOK, "✓"))
	case "ready":
		_, _ = fmt.Fprintf(out, "%s Dry run ready (target not modified)\n", style(tty, sOK, "✓"))
	case "partial":
		_, _ = fmt.Fprintf(out, "%s Migration partially completed\n", style(tty, sCmd, "!"))
	default:
		_, _ = fmt.Fprintf(out, "%s Migration %s\n", style(tty, sErr, "✗"), report.Status)
	}

	if dryRun {
		_, _ = fmt.Fprintf(out, "  History     total %d · recoverable %d · failed %d · missing parts %d\n",
			report.Total, report.Recoverable, report.Failed, report.MissingParts)
	} else {
		_, _ = fmt.Fprintf(out, "  History     total %d · imported %d · skipped %d · verified %d",
			report.Total, report.Imported, report.Skipped, report.Verified)
		if report.Failed > 0 || report.MissingParts > 0 || report.VerificationFailed > 0 {
			_, _ = fmt.Fprintf(out, " · failed %d · missing parts %d · verification failed %d",
				report.Failed, report.MissingParts, report.VerificationFailed)
		}
		_, _ = fmt.Fprintln(out)
	}

	_, _ = fmt.Fprintf(out, "  Tags        %d source · repaired %d · colors imported %d · preserved %d · verified %d",
		report.Tags, report.TagsRepaired, report.ColorsImported, report.ColorsPreserved, report.ColorsVerified)
	if report.TagFailures > 0 || report.UnsupportedTags > 0 || report.TagMismatches > 0 || report.TagRepairsSkipped > 0 || report.ColorsMissing > 0 || report.ColorFailures > 0 {
		_, _ = fmt.Fprintf(out, " · issues (tag_failures=%d unsupported=%d mismatches=%d repairs_skipped=%d colors_missing=%d color_failures=%d)",
			report.TagFailures, report.UnsupportedTags, report.TagMismatches, report.TagRepairsSkipped, report.ColorsMissing, report.ColorFailures)
	}
	_, _ = fmt.Fprintln(out)

	_, _ = fmt.Fprintf(out, "  Settings    preferences imported %d · shortcuts imported %d",
		report.PreferencesImported, report.ShortcutsImported)
	if report.PreferenceFailures > 0 || report.ShortcutsSkipped > 0 {
		_, _ = fmt.Fprintf(out, " · shortcuts skipped %d · preference failures %d",
			report.ShortcutsSkipped, report.PreferenceFailures)
	}
	_, _ = fmt.Fprintln(out)

	pFlag := ""
	if p := paths.ExplicitProfile(); p != "" && p != paths.DefaultProfileName {
		pFlag = " -p " + p
	}
	switch report.Status {
	case "complete":
		_, _ = fmt.Fprintf(out, "\nNext steps:\n")
		_, _ = fmt.Fprintf(out, "  unic%s daemon start       Keep syncing in the background\n", pFlag)
		_, _ = fmt.Fprintf(out, "  unic%s history list       Browse imported clipboard history\n", pFlag)
	case "partial":
		_, _ = fmt.Fprintf(out, "\nNext steps:\n")
		_, _ = fmt.Fprintf(out, "  unic%s migrate            Rerun to resume or repair remaining items\n", pFlag)
		_, _ = fmt.Fprintf(out, "  unic%s init --fresh --discard-pending   Abandon incomplete import and start fresh\n", pFlag)
	}
}

func hasFlagNamed(args []string, name string) bool {
	for _, a := range args {
		if a == "--" {
			return false
		}
		if a == "--"+name || a == "-"+name ||
			strings.HasPrefix(a, "--"+name+"=") || strings.HasPrefix(a, "-"+name+"=") {
			return true
		}
	}
	return false
}

func sanitizeFlagParseError(cmd string, err error) string {
	msg := err.Error()
	if strings.HasPrefix(msg, "invalid boolean value ") || strings.HasPrefix(msg, "invalid value ") {
		name := ""
		if idx := strings.Index(msg, " for flag -"); idx >= 0 {
			name = msg[idx+len(" for flag -"):]
		} else if idx := strings.Index(msg, " for -"); idx >= 0 {
			name = msg[idx+len(" for -"):]
		}
		if colon := strings.IndexByte(name, ':'); colon >= 0 {
			name = name[:colon]
		}
		if clean, ok := validFlagName(name); ok {
			return fmt.Sprintf("invalid value for --%s; run '%s --help' for usage", clean, cmd)
		}
		return fmt.Sprintf("invalid flag value; run '%s --help' for usage", cmd)
	}
	if strings.HasPrefix(msg, "flag needs an argument: -") {
		name := strings.TrimPrefix(msg, "flag needs an argument: -")
		if clean, ok := validFlagName(name); ok {
			return fmt.Sprintf("flag --%s requires an argument; run '%s --help' for usage", clean, cmd)
		}
		return fmt.Sprintf("flag requires an argument; run '%s --help' for usage", cmd)
	}
	if strings.HasPrefix(msg, "flag provided but not defined: -") {
		name := strings.TrimPrefix(msg, "flag provided but not defined: -")
		if clean, ok := validFlagName(name); ok {
			return fmt.Sprintf("unknown flag --%s; run '%s --help' for usage", clean, cmd)
		}
		return fmt.Sprintf("unknown flag; run '%s --help' for usage", cmd)
	}
	return fmt.Sprintf("invalid flags; run '%s --help' for usage", cmd)
}

func isASCIIFlagNameByte(c byte) bool {
	return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '-' || c == '_'
}

func validFlagName(raw string) (string, bool) {
	name := strings.TrimPrefix(raw, "-")
	if len(name) == 0 || len(name) > 32 {
		return "", false
	}
	for i := 0; i < len(name); i++ {
		if !isASCIIFlagNameByte(name[i]) {
			return "", false
		}
	}
	return name, true
}

func isTerminalFile(w any) bool {
	f, ok := w.(*os.File)
	return ok && f != nil && term.IsTerminal(int(f.Fd()))
}

const maxPassphraseBytes = 4096

// Reading stdin must not trap SIGINT while no passphrase has arrived. The CLI
// exits after cancellation; the reader goroutine never writes persisted state.
func readMigrationPassphrase(ctx context.Context, in *os.File, terminal bool) ([]byte, error) {
	type result struct {
		value []byte
		err   error
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if in == nil {
		return nil, errors.New("cannot read old passphrase")
	}
	var fd int
	if terminal {
		fd = int(in.Fd())
		state, err := term.GetState(fd)
		if err != nil {
			return nil, err
		}
		// ReadPassword restores on normal return; the caller must also restore
		// before process exit when cancellation returns ahead of that read.
		defer func() { _ = term.Restore(fd, state) }()
	}
	stopWake := context.AfterFunc(ctx, func() {
		_ = in.SetReadDeadline(time.Now())
	})
	defer func() {
		stopWake()
		if !terminal {
			_ = in.SetReadDeadline(time.Time{})
		}
	}()
	done := make(chan result)
	go func() {
		var r result
		if terminal {
			r.value, r.err = term.ReadPassword(fd)
		} else {
			// Fixed-capacity buffer: maxPassphraseBytes + trailing CR + one extra byte
			// to detect over-limit payloads without ever triggering append reallocation.
			buf := make([]byte, 0, maxPassphraseBytes+2)
			var b [1]byte
			defer func() {
				clear(buf[:cap(buf)])
				clear(b[:])
			}()
			tooLong := false
			for {
				n, err := in.Read(b[:])
				if n > 0 {
					ch := b[0]
					b[0] = 0
					if ch == '\n' {
						break
					}
					if len(buf) >= cap(buf) {
						tooLong = true
						break
					}
					buf = append(buf, ch)
				}
				if err != nil {
					if err != io.EOF {
						r.err = errors.New("cannot read old passphrase")
					}
					break
				}
			}
			if r.err == nil {
				if len(buf) > 0 && buf[len(buf)-1] == '\r' {
					buf[len(buf)-1] = 0
					buf = buf[:len(buf)-1]
				}
				if tooLong || len(buf) > maxPassphraseBytes {
					r.err = errors.New("cannot read old passphrase")
				} else {
					r.value = bytes.Clone(buf)
				}
			}
		}
		select {
		case done <- r:
		case <-ctx.Done():
			clear(r.value)
		}
	}()
	select {
	case r := <-done:
		return r.value, r.err
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func migrationFailure(jsonMode bool, w io.Writer, reason, message, next string) {
	if jsonMode {
		if next == "" {
			next = "unic migrate --help"
		}
		_ = json.NewEncoder(w).Encode(map[string]string{"error": message, "reason": reason, "next_command": next})
	} else {
		_, _ = fmt.Fprintln(w, errorLine(message))
	}
}

func initializeWithoutSource(ctx context.Context, target string, jsonMode bool, out, errOut io.Writer) int {
	source, err := v1migration.DefaultSource()
	if err != nil {
		migrationFailure(jsonMode, errOut, "source_unknown", "Cannot locate v1 data. Specify --source DIR and complete migration, or run 'unic init --fresh'.", "unic init --fresh")
		return 1
	}
	sources := []string{source}
	if profile := os.Getenv("UC_PROFILE"); profile != "" && !strings.ContainsAny(profile, `/\`) {
		sources = append(sources, source+"-"+profile)
	}
	for _, candidate := range sources {
		if err := v1migration.ValidateDirectories(candidate, target); err != nil {
			migrationFailure(jsonMode, errOut, "source_target_overlap", "Keep the v2 target separate from known v1 source directories.", "unic migrate --help")
			return 1
		}
		if !sourceAbsentOrEmpty(candidate) {
			migrationFailure(jsonMode, errOut, "source_present", "found existing UniClipboard v1 data; run 'unic migrate' to import it, or 'unic init --fresh' to start without v1 history.", "unic migrate")
			return 1
		}
	}
	if err := os.MkdirAll(target, 0700); err != nil {
		migrationFailure(jsonMode, errOut, "target_unavailable", "cannot create migration target directory", "unic migrate --help")
		return 1
	}
	target, err = filepath.EvalSymlinks(target)
	if err != nil {
		migrationFailure(jsonMode, errOut, "target_unavailable", "cannot resolve migration target", "unic migrate --help")
		return 1
	}
	unlock, err := servicelaunch.LockContext(ctx, target)
	if err != nil {
		reason := "lock_failed"
		if ctx.Err() != nil {
			reason = "cancelled"
		}
		migrationFailure(jsonMode, errOut, reason, "cannot acquire profile transition lock", "unic migrate")
		return 1
	}
	defer unlock()
	if ctx.Err() != nil {
		return 1
	}
	state, err := climigration.Read(target)
	if err != nil {
		migrationFailure(jsonMode, errOut, "state_invalid", err.Error(), "unic migrate")
		return 1
	}
	if state.Status == "pending" {
		migrationFailure(jsonMode, errOut, "migration_pending", "An import is pending. Rerun 'unic migrate' to complete it, or run 'unic init --fresh --discard-pending' to discard it.", "unic migrate")
		return 1
	}
	for _, candidate := range sources {
		if !sourceAbsentOrEmpty(candidate) {
			migrationFailure(jsonMode, errOut, "source_present", "found existing UniClipboard v1 data; run 'unic migrate' to import it, or 'unic init --fresh' to start without v1 history.", "unic migrate")
			return 1
		}
	}
	if state.Status != "complete" {
		probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		if err != nil {
			migrationTargetFailure(jsonMode, errOut, err)
			return 1
		}
		if ctx.Err() != nil {
			_ = probe.Close(context.Background())
			migrationFailure(jsonMode, errOut, "cancelled", ctx.Err().Error(), "unic migrate")
			return 1
		}
		if err = climigration.Write(target, climigration.State{Status: "complete", Outcome: "no_source"}); err != nil {
			_ = probe.Close(context.Background())
			migrationFailure(jsonMode, errOut, "state_unavailable", "cannot publish CLI migration state", "unic migrate")
			return 1
		}
		if err = probe.Close(context.Background()); err != nil {
			migrationFailure(jsonMode, errOut, "target_unavailable", "cannot close migration target probe", "unic migrate --help")
			return 1
		}
	}
	if state.Outcome == "" {
		state.Outcome = "no_source"
	}
	if jsonMode {
		_ = json.NewEncoder(out).Encode(map[string]string{"status": "complete", "outcome": state.Outcome, "initialization": "explicit_no_source"})
	} else {
		_, _ = fmt.Fprintln(out, "Migration initialization complete. No known v1 source exists. This profile is ready.")
	}
	return 0
}

func migrationTargetFailure(jsonMode bool, w io.Writer, err error) {
	var ee *engine.Error
	if errors.As(err, &ee) && ee.Code == engine.CodeInUse {
		migrationFailure(jsonMode, w, "in_use", "Stop the service and Desktop using the target profile, then rerun migration initialization.", "unic daemon stop")
		return
	}
	migrationFailure(jsonMode, w, "target_unavailable", "Cannot initialize the target. Check data-directory permissions, storage and available disk space, then retry.", "unic migrate --help")
}

// Inspect at most one public directory entry. Empty leftovers contain no v1
// data; a nonempty, unreadable or non-directory path must never be discarded.
func sourceAbsentOrEmpty(path string) bool {
	f, err := os.Open(path)
	if errors.Is(err, os.ErrNotExist) {
		return true
	}
	if err != nil {
		return false
	}
	defer func() { _ = f.Close() }()
	entries, err := f.ReadDir(1)
	return len(entries) == 0 && errors.Is(err, io.EOF)
}
