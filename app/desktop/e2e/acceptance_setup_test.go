package e2e

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
	"github.com/uniclipboard/unic/engine"
	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/v1migration"
	"github.com/uniclipboard/unic/internal/v1migration/v1fixture"
)

// Failure inventory is specified in docs/exec/0026-first-install-setup.md.
// This scenario uses actual encrypted source data, target storage and service IPC.
func scenarioFirstInstallSetup(t *testing.T) {
	ctx := context.Background()
	base, err := os.MkdirTemp("", "uc-setup-")
	must(t, err)
	defer func() {
		if err := os.RemoveAll(base); err != nil {
			t.Errorf("remove isolated setup profile: %v", err)
		}
	}()
	checks := []string{}
	receipt := map[string]any{"native_visual_tested": false, "system_credentials_tested": false, "fixture": "internal/v1migration/testdata/v3.json"}
	defer func() {
		receipt["checks"], receipt["status"] = checks, "passed"
		if t.Failed() {
			receipt["status"] = "failed"
		}
		raw, err := json.MarshalIndent(receipt, "", "  ")
		must(t, err)
		must(t, os.WriteFile(filepath.Join(os.Getenv("UNIC_DESKTOP_E2E_CASE_PRIVATE"), "manifest.json"), raw, 0600))
	}()
	fresh, err := setupflow.Open(filepath.Join(base, "skip"))
	must(t, err)
	if !fresh.Required() {
		t.Fatal("fresh profile bypassed setup")
	}
	found, err := setupflow.Detect(filepath.Join(base, "absent"))
	must(t, err)
	if found {
		t.Fatal("absent source detected")
	}
	must(t, fresh.Complete())
	reopened, err := setupflow.Open(filepath.Join(base, "skip"))
	must(t, err)
	if reopened.Required() {
		t.Fatal("skip did not persist completion")
	}
	old := filepath.Join(base, "existing")
	e, err := engine.New(ctx, engine.Options{DataDir: old, MigrationMode: true})
	must(t, err)
	must(t, e.Close(ctx))
	established, err := setupflow.Open(old)
	must(t, err)
	if established.Required() {
		t.Fatal("established empty profile entered setup")
	}
	checks = append(checks, "fresh, absent source, skip persistence and established empty v2 profile")
	source, dbpath, fixture, err := v1fixture.Build(filepath.Join(base, "v1"))
	must(t, err)
	found, err = setupflow.Detect(source)
	must(t, err)
	if !found {
		t.Fatal("fixture not discovered")
	}
	target := filepath.Join(base, "target")
	flow, err := setupflow.Open(target)
	must(t, err)
	must(t, flow.SelectSource(source))
	initialSourceHash := hash(t, dbpath)
	secretCalls := 0
	denied := func(string, string) ([]byte, error) { secretCalls++; return nil, v1migration.ErrUnlock }
	if _, err = flow.Run(ctx, nil, denied, nil); !errors.Is(err, v1migration.ErrUnlock) {
		t.Fatalf("credential fallback: %v", err)
	}
	if secretCalls == 0 {
		t.Fatal("credential path not exercised")
	}
	if _, err = flow.Run(ctx, []byte("wrong-passphrase"), denied, nil); !errors.Is(err, v1migration.ErrUnlock) {
		t.Fatalf("wrong password: %v", err)
	}
	interrupted, err := setupflow.Open(target)
	must(t, err)
	if !interrupted.Required() || interrupted.State().Source != source {
		t.Fatal("database creation lost unfinished setup")
	}
	cancelCtx, cancel := context.WithCancel(ctx)
	report, err := flow.Run(cancelCtx, []byte(fixture.Passphrase), denied, func(p v1migration.Progress) {
		if p.Stage == "import" && p.Processed == 3 {
			cancel()
		}
	})
	cancel()
	if !errors.Is(err, context.Canceled) || report.Status != "cancelled" || report.Imported == 0 {
		t.Fatalf("cancel did not retain commits: %+v %v", report, err)
	}
	checks = append(checks, "credential denial, wrong password, pending state after database creation and safe cancellation")
	report, err = flow.Run(ctx, []byte(fixture.Passphrase), denied, nil)
	must(t, err)
	if report.Status != "partial" || report.Skipped == 0 || report.Failed != 1 || report.Verified != 4 {
		t.Fatalf("partial/resume: %+v", report)
	}
	if hash(t, dbpath) != initialSourceHash {
		t.Fatal("source database changed during import")
	}
	receipt["partial_report"] = report
	db, err := sql.Open("sqlite", dbpath)
	must(t, err)
	_, err = db.Exec("INSERT INTO clipboard_snapshot_representation VALUES('rep-bad','event-bad','text/plain','text/plain',?,NULL,9)", []byte(fixture.Repaired))
	must(t, err)
	must(t, db.Close())
	report, err = flow.Run(ctx, []byte(fixture.Passphrase), denied, nil)
	must(t, err)
	if report.Status != "complete" || report.Imported != 1 || report.Skipped != 4 || report.Verified != 5 {
		t.Fatalf("repair retry: %+v", report)
	}
	report, err = flow.Run(ctx, []byte(fixture.Passphrase), denied, nil)
	must(t, err)
	if report.Imported != 0 || report.Skipped != 5 {
		t.Fatalf("duplicate import: %+v", report)
	}
	receipt["retry_report"] = report
	checks = append(checks, "source database preserved, partial report, repair, resume and zero-new-record retry")
	must(t, flow.Complete())
	reopened, err = setupflow.Open(target)
	must(t, err)
	if reopened.Required() {
		t.Fatal("completed migration reopened setup")
	}
	// Starting the actual service proves imported records survive release of offline ownership.
	must(t, os.WriteFile(filepath.Join(target, "config.toml"), []byte("[telemetry]\nenabled=false\n[sync]\nenabled=false\n[history]\nauto_cleanup=false\n"), 0600))
	executable := filepath.Join(base, "unicd")
	root, err := filepath.Abs("../../..")
	must(t, err)
	cmd := exec.Command("go", "build", "-race", "-o", executable, "./app/service/cmd/unicd")
	cmd.Dir = root
	if out, err := cmd.CombinedOutput(); err != nil {
		t.Fatalf("build service: %v\n%s", err, out)
	}
	socket := filepath.Join(target, "unic.sock")
	service, err := lifecycle.Start(ctx, target, socket, executable, true)
	if err != nil {
		raw, _ := os.ReadFile(filepath.Join(target, "desktop-daemon.log"))
		t.Fatalf("service readiness: %v\n%s", err, raw)
	}
	defer func() {
		if err := service.Stop(); err != nil {
			t.Errorf("stop isolated setup service: %v", err)
		}
	}()
	client := api.NewClient(socket)
	deadline, done := context.WithTimeout(ctx, 5*time.Second)
	defer done()
	page, err := client.QueryHistory(deadline, api.HistoryQuery{Limit: 50})
	must(t, err)
	if len(page.Items) != 5 {
		t.Fatalf("service history count: %d", len(page.Items))
	}
	instance, err := client.Service(deadline)
	must(t, err)
	secretCalls = 0
	_, err = flow.Run(ctx, nil, denied, nil)
	if !errors.Is(err, setupflow.ErrTargetBusy) || secretCalls != 0 {
		t.Fatalf("target conflict before credentials: %v calls=%d", err, secretCalls)
	}
	after, err := client.Service(deadline)
	must(t, err)
	if instance.ID != after.ID {
		t.Fatal("migration changed service ownership")
	}
	checks = append(checks, "real service exposes five records; concurrent import rejects before credential access and preserves owner")
	must(t, service.Stop())
	// The cleanup above is idempotent and also runs if a preceding assertion fails.
}
