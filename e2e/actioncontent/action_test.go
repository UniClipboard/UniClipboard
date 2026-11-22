// Package actioncontent exercises a real external plugin process and Engine storage.
package actioncontent

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

type actor struct{}

func (actor) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.content", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapAction}, Actions: []plugin.ActionSpec{{ID: "hash", Title: "Hash content", Accepts: []string{"text", "image", "files"}, TimeoutMS: 60000}, {ID: "wait", Title: "Wait", Accepts: []string{"text"}, TimeoutMS: 75}}}
}
func (actor) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (actor) Start(context.Context) error                            { return nil }
func (actor) Close(context.Context) error                            { return nil }
func (actor) Run(ctx context.Context, ac plugin.ActionContext) error {
	if ac.Action() == "wait" {
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
		r.Close()
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
	exe, _ := os.Executable()
	binary, _ := os.ReadFile(exe)
	digest := sha256.Sum256(binary)
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	e, err := engine.New(ctx, engine.Options{DataDir: dir, Plugins: []engine.PluginInstance{{Process: &engine.PluginProcess{Path: exe, SHA256: hex.EncodeToString(digest[:]), Spec: actor{}.Spec(), Args: []string{"-test.run=^$"}, Env: []string{"UNIC_ACTION_E2E_HELPER=1"}}}}})
	if err != nil {
		t.Fatal(err)
	}
	defer e.Close(context.Background())
	if err = e.Start(ctx); err != nil {
		t.Fatal(err)
	}
	var img bytes.Buffer
	png.Encode(&img, image.NewGray(image.Rect(0, 0, 2, 2)))
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
			defer e.ReleaseContent(ctx, h.LeaseID)
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
			defer e.ReleaseContent(ctx, lease.LeaseID)
			r, err := e.ReadContent(ctx, lease.LeaseID, lease.Content.Parts[0].ID)
			if err != nil {
				t.Fatal(err)
			}
			b, err := io.ReadAll(r)
			r.Close()
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
				if err == nil || failed.HistoryID != "" {
					t.Fatal("timeout created success")
				}
				child, stop := context.WithCancel(ctx)
				slow.RequestID = "cancel"
				slow.CancelOnContext = true
				time.AfterFunc(10*time.Millisecond, stop)
				if _, err = e.RunAction(child, slow); err == nil {
					t.Fatal("caller cancellation ignored")
				}
				if _, err = e.RunAction(ctx, slow); err == nil {
					t.Fatal("cancelled work succeeded")
				}
				req.RequestID = "after-cancel"
				if _, err = e.RunAction(ctx, req); err != nil {
					t.Fatal("plugin unusable after cancellation", err)
				}
			}

		})
	}
	if t.Failed() {
		return
	}
	dir = os.Getenv("UNIC_ACTION_RECEIPTS")
	if dir == "" {
		dir = t.TempDir()
	}
	os.MkdirAll(dir, 0700)
	b, _ := json.MarshalIndent(map[string]any{"status": "passed", "hashes": receipts, "replay": "go test -race ./e2e/actioncontent -count=1 -v"}, "", "  ")
	if err = os.WriteFile(filepath.Join(dir, "receipt.json"), b, 0600); err != nil {
		t.Fatal(err)
	}
	fmt.Println("Action content process E2E completed")
}
