package engine

import (
	"bytes"
	"context"
	"path/filepath"
	"slices"
	"sync"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

func testDataDir(t *testing.T) string {
	t.Helper()
	dir, err := filepath.EvalSymlinks(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	return dir
}

type stageTestProcessor struct {
	id    string
	stage string
	fn    func(plugin.ProcessContext) error
}

func (p *stageTestProcessor) Spec() plugin.Spec {
	return plugin.Spec{
		ID:             p.id,
		Version:        "1.0.0",
		APIVersion:     1,
		Capabilities:   []string{plugin.CapProcessor},
		ProcessorStage: p.stage,
	}
}

func (*stageTestProcessor) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*stageTestProcessor) Start(context.Context) error                            { return nil }
func (*stageTestProcessor) Close(context.Context) error                            { return nil }
func (p *stageTestProcessor) Process(_ context.Context, c plugin.ProcessContext) error {
	if p.fn != nil {
		return p.fn(c)
	}
	return nil
}

type stageTestObserver struct {
	id    string
	stage string
}

func (o *stageTestObserver) Spec() plugin.Spec {
	return plugin.Spec{
		ID:             o.id,
		Version:        "1.0.0",
		APIVersion:     1,
		Capabilities:   []string{plugin.CapObserver},
		ProcessorStage: o.stage,
	}
}

func (*stageTestObserver) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*stageTestObserver) Start(context.Context) error                            { return nil }
func (*stageTestObserver) Close(context.Context) error                            { return nil }
func (*stageTestObserver) OnEvent(context.Context, plugin.Event) error            { return nil }

type stageTestSyncBackend struct {
	id        string
	submitted chan struct{}
}

func (b *stageTestSyncBackend) Spec() plugin.Spec {
	return plugin.Spec{
		ID:               b.id,
		Version:          "1.0.0",
		APIVersion:       1,
		Capabilities:     []string{plugin.CapSyncBackend},
		TransportKind:    plugin.TransportObjectStore,
		ExternalChannels: true,
	}
}

func (*stageTestSyncBackend) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*stageTestSyncBackend) Start(context.Context) error                            { return nil }
func (*stageTestSyncBackend) Close(context.Context) error                            { return nil }
func (b *stageTestSyncBackend) Submit(_ context.Context, _ plugin.SyncView, _ plugin.SyncHost) (bool, error) {
	select {
	case b.submitted <- struct{}{}:
	default:
	}
	return false, nil
}

func TestGuardProcessorRunsBeforeEarlierAnnotateProcessorsAndSyncBackends(t *testing.T) {
	ctx := context.Background()
	var mu sync.Mutex
	var trace []string
	record := func(id string, blockSync bool) func(plugin.ProcessContext) error {
		return func(c plugin.ProcessContext) error {
			mu.Lock()
			trace = append(trace, id)
			mu.Unlock()
			if blockSync {
				if err := c.SetSync(false); err != nil {
					return err
				}
			}
			return c.AddTag(id)
		}
	}

	syncBackend := &stageTestSyncBackend{id: "ext.thirdparty_sync", submitted: make(chan struct{}, 1)}

	e, err := New(ctx, Options{
		DataDir: testDataDir(t),
		// Simulate user installing a third-party sync backend and annotate processors first,
		// then installing guard processors afterwards (appended to the end of PluginOrder).
		PluginOrder: []string{"ext.thirdparty_sync", "ext.annotate_first", "ext.annotate_second", "ext.guard_first", "ext.guard_second"},
		Plugins: []PluginInstance{
			{Plugin: syncBackend},
			{Plugin: &stageTestProcessor{id: "builtin.unlisted", stage: "", fn: record("builtin.unlisted", false)}},
			{Plugin: &stageTestProcessor{id: "ext.annotate_first", stage: "", fn: record("ext.annotate_first", false)}},
			{Plugin: &stageTestProcessor{id: "ext.annotate_second", stage: plugin.ProcessorStageAnnotate, fn: record("ext.annotate_second", false)}},
			{Plugin: &stageTestProcessor{id: "ext.guard_second", stage: plugin.ProcessorStageGuard, fn: record("ext.guard_second", false)}},
			{Plugin: &stageTestProcessor{id: "ext.guard_first", stage: plugin.ProcessorStageGuard, fn: record("ext.guard_first", true)}},
		},
	})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer func() { _ = e.Close(ctx) }()
	if err := e.Start(ctx); err != nil {
		t.Fatalf("Start: %v", err)
	}

	body := []byte("hello secret")
	lease, err := e.ImportContent(ctx, ContentInput{
		Representations: []RepresentationInput{{MIME: MimeText, Size: int64(len(body)), Reader: bytes.NewReader(body)}},
	})
	if err != nil {
		t.Fatalf("ImportContent: %v", err)
	}
	res, err := e.CommitContent(ctx, CommitRequest{RequestID: "req-stage-order", LeaseID: lease.LeaseID})
	if err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	if res.Phase != SubmissionCommitted {
		t.Fatalf("phase = %s, want committed", res.Phase)
	}
	if res.SyncDisposition != SyncNotRequested {
		t.Fatalf("sync disposition = %s, want not_requested (blocked by guard)", res.SyncDisposition)
	}
	select {
	case <-syncBackend.submitted:
		t.Fatal("sync backend was called despite guard processor blocking sync")
	default:
	}

	want := []string{"ext.guard_first", "ext.guard_second", "builtin.unlisted", "ext.annotate_first", "ext.annotate_second"}
	if !slices.Equal(trace, want) {
		t.Fatalf("processor execution order = %v, want %v", trace, want)
	}
}

func TestGuardLuaProcessorRunsBeforeAnnotateProcessors(t *testing.T) {
	ctx := context.Background()
	var mu sync.Mutex
	var trace []string

	luaGuard := `
return {
  id = "ext.lua_guard",
  version = "1.0.0",
  api_version = 1,
  capabilities = { "processor" },
  processor_stage = "guard",
  init = function() end,
  start = function() end,
  process = function(ctx)
    ctx:add_tag("guarded")
    ctx:set_sync(false)
  end,
  close = function() end,
}
`
	e, err := New(ctx, Options{
		DataDir:     testDataDir(t),
		PluginOrder: []string{"ext.annotate", "ext.lua_guard"},
		Plugins: []PluginInstance{
			{Plugin: &stageTestProcessor{id: "ext.annotate", fn: func(c plugin.ProcessContext) error {
				mu.Lock()
				if slices.Contains(c.Policy().Tags, "guarded") {
					trace = append(trace, "saw-guard")
				} else {
					trace = append(trace, "missed-guard")
				}
				mu.Unlock()
				return nil
			}}},
		},
		LuaPlugins: []LuaPluginSpec{{
			ID:             "ext.lua_guard",
			Version:        "1.0.0",
			APIVersion:     1,
			Capabilities:   []string{plugin.CapProcessor},
			ProcessorStage: plugin.ProcessorStageGuard,
			Source:         luaGuard,
		}},
	})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer func() { _ = e.Close(ctx) }()
	if err := e.Start(ctx); err != nil {
		t.Fatalf("Start: %v", err)
	}

	body := []byte("secret-token")
	lease, err := e.ImportContent(ctx, ContentInput{
		Representations: []RepresentationInput{{MIME: MimeText, Size: int64(len(body)), Reader: bytes.NewReader(body)}},
	})
	if err != nil {
		t.Fatalf("ImportContent: %v", err)
	}
	if _, err := e.CommitContent(ctx, CommitRequest{RequestID: "req-lua-guard", LeaseID: lease.LeaseID}); err != nil {
		t.Fatalf("CommitContent: %v", err)
	}
	if !slices.Equal(trace, []string{"saw-guard"}) {
		t.Fatalf("expected lua guard to run before annotate processor, got %v", trace)
	}
}

func TestProcessorStageValidation(t *testing.T) {
	ctx := context.Background()
	cases := []struct {
		name string
		in   PluginInstance
	}{
		{
			name: "unknown stage",
			in:   PluginInstance{Plugin: &stageTestProcessor{id: "ext.badstage", stage: "first"}},
		},
		{
			name: "non-processor with stage",
			in:   PluginInstance{Plugin: &stageTestObserver{id: "ext.obsstage", stage: plugin.ProcessorStageGuard}},
		},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := New(ctx, Options{DataDir: testDataDir(t), Plugins: []PluginInstance{tc.in}}); err == nil {
				t.Fatalf("expected New to reject %s", tc.name)
			}
		})
	}
}
