package main

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"time"

	"github.com/uniclipboard/unic/engine"
)

type directoryCommand struct {
	Action      string `json:"action"`
	Fixture     string `json:"fixture,omitempty"`
	Key         string `json:"key,omitempty"`
	TextFixture string `json:"text_fixture,omitempty"`
}
type directoryJob struct {
	cancel context.CancelFunc
	done   chan struct{}
	result cliResult
	err    error
	key    string
}
type directoryCheck struct {
	Case         string              `json:"case"`
	Node         string              `json:"node"`
	Result       string              `json:"result"`
	Milliseconds float64             `json:"milliseconds,omitempty"`
	Resources    *directoryResources `json:"resources,omitempty"`
	Detail       string              `json:"detail,omitempty"`
}

func (w *worker) finishDirectoryJob(ctx context.Context) (cliResult, error) {
	if w.directoryJob == nil {
		return cliResult{}, fmt.Errorf("no_directory_job")
	}
	job := w.directoryJob
	select {
	case <-job.done:
		job.cancel()
		w.directoryJob = nil
		return job.result, job.err
	case <-ctx.Done():
		return cliResult{}, ctx.Err()
	}
}
func (w *worker) directoryCommand(ctx context.Context, v directoryCommand) (any, error) {
	switch v.Action {
	case "cancel-scanning", "cancel-preparing", "mutate-preparing", "respond":
		if _, err := w.directoryCommand(ctx, directoryCommand{Action: "begin", Fixture: v.Fixture, Key: v.Key}); err != nil {
			return nil, err
		}
		stage := "preparing"
		if v.Action == "cancel-scanning" {
			stage = "scanning"
		}
		stageCtx, cancel := context.WithTimeout(ctx, 10*time.Second)
		defer cancel()
		observed := false
		for stageCtx.Err() == nil {
			s, e := w.client.Submission(stageCtx, v.Key)
			if e == nil && s.Stage == stage && s.Phase == engine.SubmissionProcessing {
				observed = true
				break
			}
			if e == nil && s.Phase != engine.SubmissionProcessing {
				break
			}
			select {
			case <-stageCtx.Done():
			case <-time.After(time.Millisecond):
			}
		}
		if !observed {
			return nil, fmt.Errorf("missed_directory_%s_stage", stage)
		}
		if v.Action == "respond" {
			started := time.Now()
			out, err := w.submitContent(ctx, submitRequest{Fixture: v.TextFixture, Key: v.Key + "-text"})
			if err != nil {
				return nil, err
			}
			if out.Result.Phase != engine.SubmissionCommitted {
				return nil, fmt.Errorf("concurrent_text_not_committed")
			}
			return float64(time.Since(started)) / float64(time.Millisecond), nil
		}
		if v.Action == "mutate-preparing" {
			f := w.fixtures[v.Fixture]
			// Opening and truncating an owned fixture after scan forces source validation.
			path := filepath.Join(f.Paths[0], "nested", "f-1023")
			if err := os.WriteFile(path, []byte("changed-after-scan"), 0600); err != nil {
				return nil, err
			}
			out, err := w.finishDirectoryJob(ctx)
			if err != nil {
				return nil, err
			}
			if out.Code == 0 {
				return nil, fmt.Errorf("changed_source_reported_success")
			}
			s, err := w.client.Submission(ctx, v.Key)
			if err != nil || s.Error == nil || s.Error.Code != engine.CodeSourceChanged {
				return nil, fmt.Errorf("changed_source_wrong_error")
			}
			return string(s.Error.Code), nil
		}
		return w.directoryCommand(ctx, directoryCommand{Action: "cancel"})
	case "crash":
		if w.daemon == nil || w.daemon.Process == nil {
			return nil, fmt.Errorf("no_owned_service")
		}
		if err := w.daemon.Process.Kill(); err != nil {
			return nil, fmt.Errorf("owned_service_crash_failed")
		}
		select {
		case <-w.daemonDone:
		case <-time.After(5 * time.Second):
			return nil, fmt.Errorf("owned_service_crash_timeout")
		}
		w.injectedCrashes++
		return true, w.stop()
	case "resources":
		return w.directoryResources(), nil
	case "begin":
		if w.directoryJob != nil {
			return nil, fmt.Errorf("directory_job_already_running")
		}
		f, ok := w.fixtures[v.Fixture]
		if !ok || f.Meta.Kind != "directory" || v.Key == "" {
			return nil, fmt.Errorf("invalid_directory_job")
		}
		jobCtx, cancel := context.WithTimeout(context.Background(), 90*time.Second)
		job := &directoryJob{cancel: cancel, done: make(chan struct{}), key: v.Key}
		paths := append([]string{}, f.Paths...)
		w.directoryJob = job
		go func() {
			defer close(job.done)
			defer cancel()
			args := []string{"send", "--to", "all", "--json", "--request-id", v.Key, "-f"}
			args = append(args, paths...)
			job.result, job.err = w.cli(jobCtx, args, nil)
		}()
		return true, nil
	case "await":
		out, err := w.finishDirectoryJob(ctx)
		if err != nil {
			return nil, err
		}
		return out.Code, nil
	case "cancel":
		if w.directoryJob == nil {
			return nil, fmt.Errorf("no_directory_job")
		}
		started := time.Now()
		if err := w.client.CancelSubmission(ctx, w.directoryJob.key); err != nil {
			return nil, fmt.Errorf("directory_cancel_failed")
		}
		out, err := w.finishDirectoryJob(ctx)
		if err != nil {
			return nil, err
		}
		if out.Code == 0 {
			return nil, fmt.Errorf("cancelled_directory_reported_success")
		}
		return float64(time.Since(started)) / float64(time.Millisecond), nil
	case "submission":
		return w.client.Submission(ctx, v.Key)
	case "export-ownership":
		if w.gate == nil {
			return nil, fmt.Errorf("receive_not_armed")
		}
		rs, err := w.newReceipts(ctx)
		if err != nil || len(rs) != 1 {
			return nil, fmt.Errorf("ownership_requires_received_tree")
		}
		dir, err := os.MkdirTemp(w.root, "user-export-")
		if err != nil {
			return nil, err
		}
		// The directory is task-owned, but product cleanup must treat its copies as user-owned.
		if _, err = w.checkedCLI(ctx, "history", "export", string(rs[0].HistoryID), "--output", dir); err != nil {
			return nil, err
		}
		if _, err = w.verifyDirectory(ctx, rs[0], dir); err != nil {
			return nil, err
		}
		repeated, err := w.cli(ctx, []string{"history", "export", string(rs[0].HistoryID), "--output", dir}, nil)
		if err != nil || repeated.Code == 0 {
			return nil, fmt.Errorf("export_overwrote_existing_root")
		}
		if _, err = w.checkedCLI(ctx, "history", "delete", string(rs[0].HistoryID), "--yes"); err != nil {
			return nil, err
		}
		if err = w.stop(); err != nil {
			return nil, err
		}
		if err = w.start(ctx); err != nil {
			return nil, err
		}
		for i, want := range w.gate.Fixture.Trees {
			got, e := inventory(filepath.Join(dir, fmt.Sprintf("r%d", i), want.Name))
			if e != nil || !reflect.DeepEqual(want, got) {
				return nil, fmt.Errorf("user_export_changed_after_cleanup")
			}
		}
		return true, os.RemoveAll(dir)
	}
	f, ok := w.fixtures[v.Fixture]
	if !ok || len(f.Paths) == 0 {
		return nil, fmt.Errorf("unknown_directory_fixture")
	}
	root := f.Paths[0]
	switch v.Action {
	case "remove-source":
		return true, os.RemoveAll(root)
	case "depth":
		p := root
		for i := 0; i < 65; i++ {
			p = filepath.Join(p, "d")
		}
		return true, os.MkdirAll(p, 0700)
	case "entries":
		for i := 0; i < 4097; i++ {
			p := filepath.Join(root, fmt.Sprintf("entry-%04d", i))
			if err := os.Mkdir(p, 0700); err != nil {
				return nil, err
			}
		}
		return true, nil
	case "files":
		for i := 0; i < 2049; i++ {
			p := filepath.Join(root, fmt.Sprintf("file-%04d", i))
			if err := os.WriteFile(p, nil, 0600); err != nil {
				return nil, err
			}
		}
		return true, nil
	case "symlink":
		return true, os.Symlink("nested", filepath.Join(root, "link"))
	case "reject":
		out, err := w.submitContent(ctx, submitRequest{Fixture: v.Fixture, Key: v.Key})
		if err == nil {
			return nil, fmt.Errorf("invalid_directory_was_accepted")
		}
		// Query only a stable error code. Never return raw CLI stderr or private paths.
		s, e := w.client.Submission(ctx, v.Key)
		code := "cli_rejected_before_admission"
		if e == nil {
			if s.Phase != engine.SubmissionFailed {
				return nil, fmt.Errorf("invalid_directory_committed")
			}
			if s.Error != nil {
				code = string(s.Error.Code)
			}
		}
		_ = out
		return code, nil
	default:
		return nil, fmt.Errorf("unknown_directory_action")
	}
}
