// Package actioncontent exercises a real external plugin process and Engine storage.
package actioncontent

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

type actor struct{}

func (actor) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.content", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapAction}, Actions: []plugin.ActionSpec{{ID: "hash", Title: "Hash content", Accepts: []string{"text", "image", "files"}, TimeoutMS: 60000}, {ID: "wait", Title: "Wait", Accepts: []string{"text"}, TimeoutMS: 75}, {ID: "waitcancel", Title: "Wait cancel", Accepts: []string{"text"}, TimeoutMS: 3000}, {ID: "close", Title: "Close parts", Accepts: []string{"files"}, TimeoutMS: 3000}, {ID: "blocked", Title: "Blocked source", Accepts: []string{"files"}, TimeoutMS: 3000}, {ID: "pid", Title: "Process identity", Accepts: []string{"files"}, TimeoutMS: 3000}}}
}
func (actor) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (actor) Start(context.Context) error                            { return nil }
func (actor) Close(context.Context) error                            { return nil }
func (actor) Run(ctx context.Context, ac plugin.ActionContext) error {
	mark := func() error { return os.WriteFile(os.Getenv("UNIC_ACTION_ADMISSION_FILE"), []byte(ac.Action()), 0600) }
	if ac.Action() == "pid" {
		return ac.SetText(strconv.Itoa(os.Getpid()))
	}
	if ac.Action() == "close" {
		source := ac.(plugin.ActionSource)
		for i := range source.Parts() {
			r, err := source.OpenPart(ctx, i)
			if err != nil {
				return err
			}
			var b [1]byte
			_, err = r.Read(b[:])
			if err != nil {
				_ = r.Close()
				return err
			}
			if err = r.Close(); err != nil {
				return err
			}
		}
		return ac.SetText(strconv.Itoa(os.Getpid()))
	}
	if ac.Action() == "blocked" {
		r, err := ac.(plugin.ActionSource).OpenPart(ctx, 0)
		if err != nil {
			return err
		}
		defer func() { _ = r.Close() }()
		if err = mark(); err != nil {
			return err
		}
		<-ctx.Done()
		return ctx.Err()
	}
	if ac.Action() == "wait" || ac.Action() == "waitcancel" {
		if err := mark(); err != nil {
			return err
		}
		<-ctx.Done()
		return ctx.Err()
	}
	source := ac.(plugin.ActionSource)
	hash := sha256.New()
	for i := range source.Parts() {
		r, e := source.OpenPart(ctx, i)
		if e != nil {
			return e
		}
		_, e = io.Copy(hash, r)
		if closeErr := r.Close(); e == nil {
			e = closeErr
		}
		if e != nil {
			return e
		}
	}
	return ac.SetText(hex.EncodeToString(hash.Sum(nil)))
}
func TestMain(m *testing.M) {
	if os.Getenv("UNIC_ACTION_E2E_HELPER") == "1" {
		if plugin.Serve(actor{}) != nil {
			os.Exit(1)
		}
		os.Exit(0)
	}
	os.Exit(m.Run())
}

func TestContentProcessE2E(t *testing.T) {
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	exe, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	binary, err := os.ReadFile(exe)
	if err != nil {
		t.Fatal(err)
	}
	digest := sha256.Sum256(binary)
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	marker := filepath.Join(dir, "admitted")
	e, err := engine.New(ctx, engine.Options{DataDir: dir, Plugins: []engine.PluginInstance{{Process: &engine.PluginProcess{Path: exe, SHA256: hex.EncodeToString(digest[:]), Spec: actor{}.Spec(), Args: []string{"-test.run=^$"}, Env: []string{"UNIC_ACTION_E2E_HELPER=1", "UNIC_ACTION_ADMISSION_FILE=" + marker}}}}})
	if err != nil {
		t.Fatal(err)
	}
	defer func() {
		if err := e.Close(context.Background()); err != nil {
			t.Error(err)
		}
	}()
	if err = e.Start(ctx); err != nil {
		t.Fatal(err)
	}
	var img bytes.Buffer
	if err := png.Encode(&img, image.NewGray(image.Rect(0, 0, 2, 2))); err != nil {
		t.Fatal(err)
	}
	blob := bytes.Repeat([]byte{0, 1, 2, 255}, 600000)
	cases := []struct {
		name string
		in   engine.ContentInput
		data []byte
	}{
		{"text", engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: engine.MimeText, Reader: bytes.NewReader([]byte("synthetic")), Size: 9}}}, []byte("synthetic")},
		{"image", engine.ContentInput{Representations: []engine.RepresentationInput{{MIME: "image/png", Reader: bytes.NewReader(img.Bytes()), Size: int64(img.Len())}}}, img.Bytes()},
		{"files", engine.ContentInput{Files: []engine.FileInput{{Name: "large.bin", Reader: bytes.NewReader(blob), Size: int64(len(blob))}, {Name: "empty.txt", Reader: bytes.NewReader(nil), Size: 0}}}, blob},
	}
	receipts := map[string]string{}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h, err := e.ImportContent(ctx, tc.in)
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := e.ReleaseContent(ctx, h.LeaseID); err != nil {
					t.Error(err)
				}
			}()
			src, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: engine.RequestID("source-" + tc.name), LeaseID: h.LeaseID})
			if err != nil {
				t.Fatal(err)
			}
			req := engine.RunActionRequest{RequestID: engine.RequestID("hash-" + tc.name), Key: "example.content/hash", HistoryID: src.HistoryID}
			result, err := e.RunAction(ctx, req)
			if err != nil {
				t.Fatal(err)
			}
			again, err := e.RunAction(ctx, req)
			if err != nil || again.HistoryID != result.HistoryID {
				t.Fatal("idempotency failed", err)
			}
			item, err := e.GetItem(ctx, result.HistoryID)
			if err != nil || item.Origin.Kind != engine.OriginKindAction {
				t.Fatal("result origin", err)
			}
			lease, err := e.RetainHistoryContent(ctx, result.HistoryID)
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := e.ReleaseContent(ctx, lease.LeaseID); err != nil {
					t.Error(err)
				}
			}()
			r, err := e.ReadContent(ctx, lease.LeaseID, lease.Content.Parts[0].ID)
			if err != nil {
				t.Fatal(err)
			}
			b, err := io.ReadAll(r)
			if closeErr := r.Close(); err == nil {
				err = closeErr
			}
			if err != nil {
				t.Fatal(err)
			}
			sum := sha256.Sum256(tc.data)
			want := hex.EncodeToString(sum[:])
			if string(b) != want {
				t.Fatal("content hash mismatch")
			}
			if _, err = e.GetItem(ctx, src.HistoryID); err != nil {
				t.Fatal("source lost")
			}
			receipts[tc.name] = want
			if tc.name == "text" {
				slow := engine.RunActionRequest{RequestID: "timeout", Key: "example.content/wait", HistoryID: src.HistoryID}
				failed, err := e.RunAction(ctx, slow)
				if !hasCode(err, engine.CodeTimeout) || failed.HistoryID != "" {
					t.Fatal("deadline must return timeout", err)
				}
				if err := os.Remove(marker); err != nil {
					t.Fatal(err)
				}
				child, stop := context.WithCancel(ctx)
				defer stop()
				slow.RequestID = "cancel"
				slow.Key = "example.content/waitcancel"
				slow.CancelOnContext = true
				done := make(chan error, 1)
				go func() { _, err := e.RunAction(child, slow); done <- err }()
				waitAdmission(t, ctx, marker)
				stop()
				if err := <-done; !hasCode(err, engine.CodeCancelled) {
					t.Fatal("caller cancellation", err)
				}
				if _, err = e.RunAction(ctx, slow); !hasCode(err, engine.CodeCancelled) {
					t.Fatal("admitted cancellation must be cached", err)
				}
				req.RequestID = "after-cancel"
				if _, err = e.RunAction(ctx, req); err != nil {
					t.Fatal("plugin unusable after cancellation", err)
				}
			}

		})
	}

	t.Run("early-close-and-cancel-preserve-process", func(t *testing.T) {
		var files []engine.FileInput
		for i := 0; i < 6; i++ {
			files = append(files, engine.FileInput{Name: fmt.Sprintf("part-%d", i), Reader: bytes.NewReader(blob), Size: int64(len(blob))})
		}
		h, err := e.ImportContent(ctx, engine.ContentInput{Files: files})
		if err != nil {
			t.Fatal(err)
		}
		defer func() { _ = e.ReleaseContent(ctx, h.LeaseID) }()
		src, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "six-parts", LeaseID: h.LeaseID})
		if err != nil {
			t.Fatal(err)
		}
		readPID := func(id engine.HistoryID) string {
			t.Helper()
			h, err := e.RetainHistoryContent(ctx, id)
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = e.ReleaseContent(ctx, h.LeaseID) }()
			r, err := e.ReadContent(ctx, h.LeaseID, h.Content.Parts[0].ID)
			if err != nil {
				t.Fatal(err)
			}
			b, err := io.ReadAll(r)
			if closeErr := r.Close(); err == nil {
				err = closeErr
			}
			if err != nil {
				t.Fatal(err)
			}
			return string(b)
		}
		first, err := e.RunAction(ctx, engine.RunActionRequest{RequestID: "first-pid", Key: "example.content/pid", HistoryID: src.HistoryID})
		if err != nil {
			t.Fatal(err)
		}
		pid := readPID(first.HistoryID)
		closeResult, err := e.RunAction(ctx, engine.RunActionRequest{RequestID: "early-close", Key: "example.content/close", HistoryID: src.HistoryID})
		if err != nil {
			t.Fatal("closing parts must release host slots", err)
		}
		if readPID(closeResult.HistoryID) != pid {
			t.Fatal("early close restarted process")
		}
		if err := os.Remove(marker); err != nil {
			t.Fatal(err)
		}
		child, stop := context.WithCancel(ctx)
		defer stop()
		request := engine.RunActionRequest{RequestID: "blocked-cancel", Key: "example.content/blocked", HistoryID: src.HistoryID, CancelOnContext: true}
		done := make(chan error, 1)
		go func() { _, err := e.RunAction(child, request); done <- err }()
		waitAdmission(t, ctx, marker)
		stop()
		if err := <-done; !hasCode(err, engine.CodeCancelled) {
			t.Fatal(err)
		}
		if _, err := e.RunAction(ctx, request); !hasCode(err, engine.CodeCancelled) {
			t.Fatal("cancellation not cached", err)
		}
		after, err := e.RunAction(ctx, engine.RunActionRequest{RequestID: "last-pid", Key: "example.content/pid", HistoryID: src.HistoryID})
		if err != nil {
			t.Fatal(err)
		}
		if readPID(after.HistoryID) != pid {
			t.Fatal("cancellation restarted process")
		}
	})
	if t.Failed() {
		return
	}
	dir = os.Getenv("UNIC_ACTION_RECEIPTS")
	if dir == "" {
		dir = t.TempDir()
	}
	if err := os.MkdirAll(dir, 0700); err != nil {
		t.Fatal(err)
	}
	b, err := json.MarshalIndent(map[string]any{"status": "passed", "hashes": receipts, "replay": "go test -race ./e2e/actioncontent -count=1 -v"}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err = os.WriteFile(filepath.Join(dir, "receipt.json"), b, 0600); err != nil {
		t.Fatal(err)
	}
	fmt.Println("Action content process E2E completed")
}

func hasCode(err error, code engine.Code) bool {
	var e *engine.Error
	return errors.As(err, &e) && e.Code == code
}
func waitAdmission(t *testing.T, ctx context.Context, path string) {
	t.Helper()
	ticker := time.NewTicker(time.Millisecond)
	defer ticker.Stop()
	for {
		if _, err := os.Stat(path); err == nil {
			return
		} else if !os.IsNotExist(err) {
			t.Fatal(err)
		}
		select {
		case <-ctx.Done():
			t.Fatal("admission not observed")
		case <-ticker.C:
		}
	}
}
