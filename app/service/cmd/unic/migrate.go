package main

import (
	"bufio"
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

	"golang.org/x/term"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/climigration"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/servicelaunch"
	"github.com/uniclipboard/unic/internal/v1migration"
)

const migrationGuide = `UniClipboard v1 to v2 history migration

First use of each CLI data directory requires explicit migration initialization.
Ordinary commands exit 6 (migration_required) until initialization completes.
Import and verification must be complete. Dry-run, partial, failure and cancel
keep the profile blocked. Help and version remain available.

If this computer has no v1 source, initialize explicitly:
  unic migrate --initialize-empty --json
This checks known v1 locations and refuses an existing source or pending import.
It is not a skip option. For a custom v1 location, use --source DIR to import it.

v2 cannot sync with v1 or directly use its database, settings or device identity.
Upgrade all devices to v2 and pair them again. Configure v2 settings separately.

1. Back up your v1 data. Keep the backup until you have checked the v2 history.
2. Quit v1 and stop unicd for the target profile. Migration does not stop them.
3. Check recoverable history without writing the target:
     unic migrate --dry-run --json
4. Import history and verify the copied content:
     unic migrate --json
5. Check the report and v2 history before starting unicd.

Default source: the local v1 desktop data directory.
Default target: UNIC_HOME or the normal unic data directory.
Custom directories: add --source DIR --target DIR to both commands.
Named profiles: add --source DIR --service UniClipboard-PROFILE for that profile.

Migration imports supported text, HTML, images, files and tags. It does not
import settings, device identities, trust or favorites. Missing or damaged
content is reported as partial. Exit codes: 0 ready/complete, 3 partial, 1 failed.
The source remains unchanged. Rerun the same command after an interruption;
successful records are not duplicated. A dry run is not a completed import.

Every import automatically fills missing source tags on unedited imported
records and fills missing tag colors. Existing target color overrides, edited
records and deletions are preserved. The desktop uses the same migration path.
Rerun the normal command to resume or repair; no repair option is needed.
Check tag_repairs_skipped, color_failures and colors_missing in the report.
Resolve edited records manually in v2. Repeated migration is idempotent.
Built-in link/image/directory/code tags retain their canonical v2 identities.
Favorites are unsupported and reported; they are not silently imported.

Allow the system credential prompt or enter the old passphrase when requested.
For automation use --passphrase-stdin. Never put a passphrase in arguments or
environment variables. Custom or portable v1 profiles require --source DIR.
`

// runMigration imports local UniClipboard v1 history in-process, without
// starting networking or clipboard integration. It returns the exit code:
// 0 complete, 3 partial, 1 failed, 2 usage.
func runMigration(args []string, in *os.File, out, errOut io.Writer) int {
	if len(args) == 0 || len(args) == 1 && (args[0] == "--help" || args[0] == "-h" || args[0] == "help") {
		if _, err := fmt.Fprint(out, migrationGuide); err != nil {
			return 1
		}
		return 0
	}
	fs := flag.NewFlagSet("unic migrate", flag.ContinueOnError)
	fs.SetOutput(errOut)
	source := fs.String("source", "", "v1 data directory (default: local desktop profile)")
	target := fs.String("target", "", "unic data directory (default: UNIC_HOME or normal unic directory)")
	service := fs.String("service", "UniClipboard", "old credential service; named profiles use UniClipboard-PROFILE")
	initializeEmpty := fs.Bool("initialize-empty", false, "explicitly initialize when no known v1 source exists and no import is pending")
	dry := fs.Bool("dry-run", false, "read and authenticate content without changing the target")
	jsonMode := fs.Bool("json", false, "emit JSON progress and final report")
	stdinPassword := fs.Bool("passphrase-stdin", false, "read old passphrase as one line from stdin; never pass it as an argument")
	if err := fs.Parse(args); err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return 0
		}
		return 2
	}
	if fs.NArg() != 0 {
		_, _ = fmt.Fprintln(errOut, "unexpected positional arguments")
		return 2
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt)
	defer cancel()
	if *target == "" {
		var err error
		*target, err = paths.ResolveDir()
		if err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
	}
	targetPath, err := filepath.Abs(*target)
	if err != nil {
		_, _ = fmt.Fprintln(errOut, "cannot resolve migration target")
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
			_, _ = fmt.Fprintln(errOut, "--initialize-empty only accepts --target and --json")
			return 2
		}
		return initializeWithoutSource(ctx, *target, *jsonMode, out, errOut)
	}
	if *source == "" {
		*source, err = v1migration.DefaultSource()
		if err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
	}
	// Validate only public structure before recording an import attempt. A typo
	// must not permanently strand a user who has no legacy data.
	st, err := os.Stat(filepath.Join(*source, "vault", "keyslot.json"))
	if err != nil || !st.Mode().IsRegular() || st.Size() > 8<<20 {
		migrationFailure(*jsonMode, errOut, "source_invalid", "The v1 source is missing or invalid. Use --source DIR, or explicitly use --initialize-empty if no v1 source exists.")
		return 1
	}
	if err = v1migration.ValidateDirectories(*source, *target); err != nil {
		migrationFailure(*jsonMode, errOut, "source_target_overlap", "Source and target must be separate directories.")
		return 1
	}
	var pending climigration.State
	if !*dry {
		if err = os.MkdirAll(*target, 0700); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
		*target, err = filepath.EvalSymlinks(*target)
		if err != nil {
			_, _ = fmt.Fprintln(errOut, "cannot resolve migration target")
			return 1
		}
		unlock, err := servicelaunch.LockContext(ctx, *target)
		if err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
		defer unlock()
		if err = ctx.Err(); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
		pending, err = climigration.Begin(*target, *source)
		if err != nil {
			reason := "state_unavailable"
			if errors.Is(err, climigration.ErrSourceMismatch) {
				reason = "source_mismatch"
			}
			migrationFailure(*jsonMode, errOut, reason, err.Error())
			return 1
		}
		probe, err := engine.New(ctx, engine.Options{DataDir: *target, MigrationMode: true})
		if err != nil {
			migrationTargetFailure(*jsonMode, errOut, err)
			return 1
		}
		if err = probe.Close(context.Background()); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
	}
	var password []byte
	if *stdinPassword {
		var err error
		password, err = readMigrationPassphrase(ctx, in, false)
		if err != nil {
			_, _ = fmt.Fprintln(errOut, "cannot read old passphrase; migration remains incomplete")
			return 1
		}
	}
	defer func() { clear(password) }()
	if *jsonMode {
		_ = json.NewEncoder(out).Encode(map[string]string{"stage": "unlock"})
	} else {
		_, _ = fmt.Fprintln(errOut, "Unlocking old history; allow the system credential prompt or enter the old passphrase if requested.")
	}
	s, err := v1migration.Open(ctx, *source, *service, password, func(service, account string) ([]byte, error) {
		return v1migration.ReadSecretContext(ctx, service, account)
	})
	if errors.Is(err, v1migration.ErrUnlock) && !*stdinPassword && term.IsTerminal(int(in.Fd())) {
		if *jsonMode {
			_ = json.NewEncoder(out).Encode(map[string]string{"stage": "needs_passphrase"})
		}
		_, _ = fmt.Fprint(errOut, "Old UniClipboard passphrase: ")
		password, err = readMigrationPassphrase(ctx, in, true)
		_, _ = fmt.Fprintln(errOut)
		if err == nil {
			s, err = v1migration.Open(ctx, *source, *service, password, nil)
		}
	}
	clear(password)
	if err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	defer func() { _ = s.Close() }()
	if !*dry {
		_, err = climigration.Bind(*target, pending, s.ID())
		if err != nil {
			reason := "state_unavailable"
			if errors.Is(err, climigration.ErrSourceMismatch) {
				reason = "source_mismatch"
			}
			migrationFailure(*jsonMode, errOut, reason, err.Error())
			return 1
		}
	}
	var lastStage string
	report, err := s.Run(ctx, v1migration.RunOptions{Target: *target, DryRun: *dry, Progress: func(p v1migration.Progress) {
		if p.Stage != lastStage || p.Processed == 1 || p.Processed%25 == 0 || p.Processed == p.Total {
			if *jsonMode {
				_ = json.NewEncoder(out).Encode(p)
			} else {
				_, _ = fmt.Fprintf(errOut, "%s %d/%d\n", p.Stage, p.Processed, p.Total)
			}
			lastStage = p.Stage
		}
	}})
	if *jsonMode {
		_ = json.NewEncoder(out).Encode(report)
	} else {
		_, _ = fmt.Fprintf(out, "%s: total=%d recoverable=%d imported=%d skipped=%d failed=%d missing_parts=%d verified=%d verification_failed=%d tags=%d tag_failures=%d unsupported_tags=%d tag_mismatches=%d tags_repaired=%d tag_repairs_skipped=%d colors_imported=%d colors_preserved=%d colors_missing=%d colors_verified=%d color_failures=%d\n",
			report.Status, report.Total, report.Recoverable, report.Imported, report.Skipped, report.Failed, report.MissingParts, report.Verified, report.VerificationFailed, report.Tags, report.TagFailures, report.UnsupportedTags, report.TagMismatches, report.TagsRepaired, report.TagRepairsSkipped, report.ColorsImported, report.ColorsPreserved, report.ColorsMissing, report.ColorsVerified, report.ColorFailures)
	}
	if err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	if report.Status == "partial" {
		return 3
	}
	if !*dry {
		if report.Status != "complete" || ctx.Err() != nil {
			return 1
		}
		if err = s.Close(); err != nil {
			_, _ = fmt.Fprintln(errOut, "cannot close source; migration remains incomplete")
			return 1
		}
		if err = climigration.Write(*target, climigration.State{Status: "complete", Outcome: "imported", SourceID: s.ID()}); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
	}
	return 0
}

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
	if terminal {
		state, err := term.GetState(int(in.Fd()))
		if err != nil {
			return nil, err
		}
		// ReadPassword restores on normal return; the caller must also restore
		// before process exit when cancellation returns ahead of that read.
		defer func() { _ = term.Restore(int(in.Fd()), state) }()
	}
	done := make(chan result)
	go func() {
		var r result
		if terminal {
			r.value, r.err = term.ReadPassword(int(in.Fd()))
		} else {
			line, err := bufio.NewReader(io.LimitReader(in, 4097)).ReadString('\n')
			if err != nil && err != io.EOF || len(line) > 4096 {
				r.err = errors.New("cannot read old passphrase")
			} else {
				r.value = []byte(strings.TrimSuffix(strings.TrimSuffix(line, "\n"), "\r"))
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

func migrationFailure(jsonMode bool, w io.Writer, reason, message string) {
	if jsonMode {
		_ = json.NewEncoder(w).Encode(map[string]string{"error": message, "reason": reason, "next_command": "unic migrate --help"})
	} else {
		_, _ = fmt.Fprintln(w, message)
	}
}

func initializeWithoutSource(ctx context.Context, target string, jsonMode bool, out, errOut io.Writer) int {
	source, err := v1migration.DefaultSource()
	if err != nil {
		migrationFailure(jsonMode, errOut, "source_unknown", "Cannot locate v1 data. Specify --source DIR and complete migration.")
		return 1
	}
	sources := []string{source}
	if profile := os.Getenv("UC_PROFILE"); profile != "" && !strings.ContainsAny(profile, `/\`) {
		sources = append(sources, source+"-"+profile)
	}
	for _, candidate := range sources {
		if err := v1migration.ValidateDirectories(candidate, target); err != nil {
			migrationFailure(jsonMode, errOut, "source_target_overlap", "Keep the v2 target separate from known v1 source directories.")
			return 1
		}
		_, err := os.Stat(candidate)
		if err == nil || !errors.Is(err, os.ErrNotExist) {
			migrationFailure(jsonMode, errOut, "source_present", "A v1 source exists or cannot be checked. Complete unic migrate --source DIR before using this profile.")
			return 1
		}
	}
	if err := os.MkdirAll(target, 0700); err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	target, err = filepath.EvalSymlinks(target)
	if err != nil {
		_, _ = fmt.Fprintln(errOut, "cannot resolve migration target")
		return 1
	}
	unlock, err := servicelaunch.LockContext(ctx, target)
	if err != nil {
		_, _ = fmt.Fprintln(errOut, err)
		return 1
	}
	defer unlock()
	if ctx.Err() != nil {
		return 1
	}
	state, err := climigration.Read(target)
	if err != nil {
		migrationFailure(jsonMode, errOut, "state_invalid", err.Error())
		return 1
	}
	if state.Status == "pending" {
		migrationFailure(jsonMode, errOut, "migration_pending", "An import is pending. Rerun unic migrate --source DIR to complete and verify it.")
		return 1
	}
	for _, candidate := range sources {
		if _, err := os.Stat(candidate); err == nil || !errors.Is(err, os.ErrNotExist) {
			migrationFailure(jsonMode, errOut, "source_present", "A v1 source exists or cannot be checked. Complete unic migrate --source DIR before using this profile.")
			return 1
		}
	}
	if state.Status != "complete" {
		probe, err := engine.New(ctx, engine.Options{DataDir: target, MigrationMode: true})
		if err != nil {
			migrationTargetFailure(jsonMode, errOut, err)
			return 1
		}
		if err = probe.Close(context.Background()); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
			return 1
		}
		if ctx.Err() != nil {
			return 1
		}
		if err = climigration.Write(target, climigration.State{Status: "complete", Outcome: "no_source"}); err != nil {
			_, _ = fmt.Fprintln(errOut, err)
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
		migrationFailure(jsonMode, w, "in_use", "Stop the service and Desktop using the target profile, then rerun migration initialization.")
		return
	}
	migrationFailure(jsonMode, w, "target_unavailable", "Cannot initialize the target. Check data-directory permissions, storage and available disk space, then retry.")
}
