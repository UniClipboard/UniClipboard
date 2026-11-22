package actioncontent

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/plugin"
)

type brokenSource struct{ mode atomic.Int32 }

func (*brokenSource) PersistentRef() string { return "synthetic-action-source" }
func (s *brokenSource) Open(context.Context) (io.ReadCloser, engine.FileMeta, error) {
	meta := engine.FileMeta{Size: 4, Version: "synthetic-v1"}
	switch s.mode.Load() {
	case 1:
		return nil, meta, os.ErrPermission
	case 2:
		return &badRead{}, meta, nil
	case 3:
		return io.NopCloser(bytes.NewBufferString("x")), meta, nil
	}
	return io.NopCloser(bytes.NewBufferString("data")), meta, nil
}

type badRead struct{}

func (*badRead) Read([]byte) (int, error) { return 0, io.ErrUnexpectedEOF }
func (*badRead) Close() error             { return nil }

type swallowingActor struct{}

func (swallowingActor) Spec() plugin.Spec {
	return plugin.Spec{ID: "example.swallow", Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapAction}, Actions: []plugin.ActionSpec{{ID: "swallow", Title: "Swallow source error", Accepts: []string{"files"}}}}
}
func (swallowingActor) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (swallowingActor) Start(context.Context) error                            { return nil }
func (swallowingActor) Close(context.Context) error                            { return nil }
func (swallowingActor) Run(ctx context.Context, ac plugin.ActionContext) error {
	r, err := ac.(plugin.ActionSource).OpenPart(ctx, 0)
	if err == nil {
		_, _ = io.Copy(io.Discard, r)
		_ = r.Close()
	}
	_ = ac.SetText("must not commit after a source failure")
	return nil
}
func TestSwallowedSourceFailureE2E(t *testing.T) {
	for _, tc := range []struct {
		name string
		mode int32
	}{{"open", 1}, {"read", 2}, {"short-eof", 3}} {
		t.Run(tc.name, func(t *testing.T) {
			ctx := context.Background()
			dir, err := filepath.EvalSymlinks(t.TempDir())
			if err != nil {
				t.Fatal(err)
			}
			source := &brokenSource{}
			e, err := engine.New(ctx, engine.Options{DataDir: dir, ResolveFileRef: func(string) engine.FileSource { return source }, Plugins: []engine.PluginInstance{{Plugin: swallowingActor{}}}})
			if err != nil {
				t.Fatal(err)
			}
			defer func() {
				if err := e.Close(ctx); err != nil {
					t.Error(err)
				}
			}()
			if err = e.Start(ctx); err != nil {
				t.Fatal(err)
			}
			h, err := e.ImportContent(ctx, engine.ContentInput{Files: []engine.FileInput{{Name: "synthetic.bin", Size: 4, Source: source}}})
			if err != nil {
				t.Fatal(err)
			}
			defer func() { _ = e.ReleaseContent(ctx, h.LeaseID) }()
			src, err := e.CommitContent(ctx, engine.CommitRequest{RequestID: "source", LeaseID: h.LeaseID})
			if err != nil {
				t.Fatal(err)
			}
			source.mode.Store(tc.mode)
			result, err := e.RunAction(ctx, engine.RunActionRequest{RequestID: "swallowed", Key: "example.swallow/swallow", HistoryID: src.HistoryID})
			var failure *engine.Error
			if !errors.As(err, &failure) || result.HistoryID != "" {
				t.Fatalf("swallowed %s error committed a success: %v", tc.name, err)
			}
			if _, err = e.GetItem(ctx, src.HistoryID); err != nil {
				t.Fatal("source lost", err)
			}
		})
	}
}
