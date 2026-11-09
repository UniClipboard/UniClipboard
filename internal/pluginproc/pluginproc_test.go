package pluginproc

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugin/rpc"
)

// helperPlugin is the plugin side of the real-subprocess tests. The test
// binary re-executes itself in this role when UNIC_TEST_PLUGIN is set.
type helperPlugin struct{ host plugin.Host }

func helperSpec() plugin.Spec {
	return plugin.Spec{ID: "example.helper", Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapProcessor, plugin.CapAction, plugin.CapObserver},
		Actions: []plugin.ActionSpec{
			{ID: "upper", Title: "Upper", Accepts: []string{"text"}},
			{ID: "big", Title: "Big", Accepts: []string{"text"}},
			{ID: "none", Title: "None", Accepts: []string{"text"}},
		}}
}

func (h *helperPlugin) Spec() plugin.Spec { return helperSpec() }
func (h *helperPlugin) Init(_ context.Context, host plugin.Host, cfg plugin.Config) error {
	h.host = host
	if _, bad := cfg["bad"]; bad {
		return plugin.ErrInvalidConfig
	}
	return nil
}
func (h *helperPlugin) Start(context.Context) error { return nil }
func (h *helperPlugin) Close(context.Context) error { return nil }
func (h *helperPlugin) Process(ctx context.Context, pc plugin.ProcessContext) error {
	text := pc.Item().Text
	switch {
	case strings.Contains(text, "panic"):
		panic("boom " + text)
	case strings.Contains(text, "crash"):
		os.Exit(3)
	case strings.Contains(text, "hang"):
		time.Sleep(time.Hour) // ignores ctx on purpose
	case strings.Contains(text, "fail"):
		return errors.New("secret text " + text)
	case strings.Contains(text, "notify"):
		h.host.Notify(ctx, "hello from plugin")
	case strings.Contains(text, "badtag"):
		_ = pc.AddTag("")
		return nil
	}
	if strings.Contains(text, "@") {
		if err := pc.AddTag("email"); err != nil {
			return err
		}
	}
	if strings.Contains(text, "drop") {
		_ = pc.RemoveTag("old")
		_ = pc.SetSync(false)
	}
	if strings.Contains(text, "expire") {
		_ = pc.SetExpire(90 * time.Minute)
	}
	return nil
}
func (h *helperPlugin) Run(_ context.Context, ac plugin.ActionContext) error {
	switch ac.Action() {
	case "upper":
		return ac.SetText(strings.ToUpper(ac.Item().Text))
	case "big":
		return ac.SetText(strings.Repeat("0123456789abcdef", 3<<16))
	}
	return nil // "none": forgets to set a result
}

func (h *helperPlugin) OnEvent(ctx context.Context, ev plugin.Event) error {
	if ev.ItemID == "fail" {
		return errors.New("secret")
	}
	h.host.Notify(ctx, "event "+ev.Kind+" "+ev.ItemID)
	return nil
}

func TestMain(m *testing.M) {
	switch os.Getenv("UNIC_TEST_PLUGIN") {
	case "":
		os.Exit(m.Run())
	case "serve":
		_ = plugin.Serve(&helperPlugin{})
		os.Exit(0)
	case "ignore-eof":
		go func() { _ = plugin.Serve(&helperPlugin{}) }()
		select {}
	case "peer":
		_ = plugin.Serve(&peerHelper{})
		os.Exit(0)
	case "garbage":
		_, _ = os.Stdout.WriteString("this is not a frame at all, just text")
		os.Exit(0)
	case "other-identity":
		_ = plugin.Serve(&otherPlugin{})
		os.Exit(0)
	}
}

type otherPlugin struct{ helperPlugin }

func (*otherPlugin) Spec() plugin.Spec { s := helperSpec(); s.Version = "9.9.9"; return s }

var exeDigest = sync.OnceValues(func() (string, string) {
	path, err := os.Executable()
	if err != nil {
		panic(err)
	}
	b, err := os.ReadFile(path)
	if err != nil {
		panic(err)
	}
	sum := sha256.Sum256(b)
	return path, hex.EncodeToString(sum[:])
})

func exeSpec(t *testing.T, mode string) Spec {
	t.Helper()
	path, digest := exeDigest()
	return Spec{Path: path, SHA256: digest, Args: []string{"-test.run=^$"}, Env: []string{"UNIC_TEST_PLUGIN=" + mode},
		Dir: t.TempDir(), Expected: helperSpec()}
}

type fakeProc struct {
	event  string
	item   plugin.Item
	policy plugin.Policy
}

func (f *fakeProc) Event() string         { return f.event }
func (f *fakeProc) Item() plugin.Item     { return f.item }
func (f *fakeProc) Policy() plugin.Policy { return f.policy }
func (f *fakeProc) AddTag(t string) error {
	if !slices.Contains(f.policy.Tags, t) {
		f.policy.Tags = append(f.policy.Tags, t)
	}
	return nil
}
func (f *fakeProc) RemoveTag(t string) error {
	f.policy.Tags = slices.DeleteFunc(f.policy.Tags, func(v string) bool { return v == t })
	return nil
}
func (f *fakeProc) SetPersist(b bool) error { f.policy.Persist = b; return nil }
func (f *fakeProc) SetSync(b bool) error    { f.policy.Sync = b; return nil }
func (f *fakeProc) SetExpire(d time.Duration) error {
	f.policy.ExpireAfter = d
	return nil
}

func newProc(text string) *fakeProc {
	return &fakeProc{event: plugin.EventCaptured, item: plugin.Item{ID: "item-1", Type: "text", Text: text, Formats: []string{"text/plain"}, Origin: plugin.Origin{Kind: "local_system"}},
		policy: plugin.Policy{Persist: true, Sync: true, Tags: []string{"old"}}}
}

type fakeAction struct {
	id   string
	item plugin.Item
	text string
	set  int
}

func (f *fakeAction) Action() string    { return f.id }
func (f *fakeAction) Item() plugin.Item { return f.item }
func (f *fakeAction) SetText(s string) error {
	f.text, f.set = s, f.set+1
	return nil
}

type fakeHost struct {
	mu   sync.Mutex
	msgs []string
}

func (h *fakeHost) Notify(_ context.Context, m string) {
	h.mu.Lock()
	h.msgs = append(h.msgs, m)
	h.mu.Unlock()
}

func start(t *testing.T, spec Spec, opts Options, cfg plugin.Config) (plugin.Plugin, *fakeHost) {
	t.Helper()
	p := New(spec, opts)
	host := &fakeHost{}
	ctx := context.Background()
	if err := p.Init(ctx, host, cfg); err != nil {
		t.Fatalf("Init: %v", err)
	}
	if err := p.Start(ctx); err != nil {
		t.Fatalf("Start: %v", err)
	}
	t.Cleanup(func() { _ = p.Close(context.Background()) })
	return p, host
}

func TestProcessorAnnotates(t *testing.T) {
	p, _ := start(t, exeSpec(t, "serve"), Options{}, nil)
	proc := p.(plugin.Processor)

	pc := newProc("write to a@b.example")
	if err := proc.Process(context.Background(), pc); err != nil {
		t.Fatal(err)
	}
	if !slices.Equal(pc.policy.Tags, []string{"old", "email"}) {
		t.Fatalf("tags %v", pc.policy.Tags)
	}

	pc = newProc("drop it, expire soon")
	if err := proc.Process(context.Background(), pc); err != nil {
		t.Fatal(err)
	}
	if pc.policy.Sync || len(pc.policy.Tags) != 0 || pc.policy.ExpireAfter != 90*time.Minute || !pc.policy.Persist {
		t.Fatalf("policy %+v", pc.policy)
	}
}

func TestActions(t *testing.T) {
	p, _ := start(t, exeSpec(t, "serve"), Options{}, nil)
	act := p.(plugin.Actor)
	ctx := context.Background()

	ac := &fakeAction{id: "upper", item: newProc("hello 你好").item}
	if err := act.Run(ctx, ac); err != nil || ac.text != "HELLO 你好" || ac.set != 1 {
		t.Fatalf("%q %d %v", ac.text, ac.set, err)
	}
	// 3 MiB result crosses several flow-control windows.
	ac = &fakeAction{id: "big", item: newProc("x").item}
	if err := act.Run(ctx, ac); err != nil || len(ac.text) != 3<<20 {
		t.Fatalf("%d %v", len(ac.text), err)
	}
	// an action that never sets a result fails and leaves nothing behind.
	ac = &fakeAction{id: "none", item: newProc("x").item}
	if err := act.Run(ctx, ac); err == nil || ac.set != 0 {
		t.Fatalf("err %v set %d", err, ac.set)
	}
	// the session is still healthy afterwards.
	ac = &fakeAction{id: "upper", item: newProc("again").item}
	if err := act.Run(ctx, ac); err != nil || ac.text != "AGAIN" {
		t.Fatalf("%q %v", ac.text, err)
	}
}

func TestLargeInputText(t *testing.T) {
	p, _ := start(t, exeSpec(t, "serve"), Options{ProcessTimeout: 5 * time.Second}, nil)
	pc := newProc(strings.Repeat("a@b ", 1<<19)) // 2 MiB
	if err := p.(plugin.Processor).Process(context.Background(), pc); err != nil || !slices.Contains(pc.policy.Tags, "email") {
		t.Fatalf("%v %v", pc.policy.Tags, err)
	}
}

func TestNotifyReachesHost(t *testing.T) {
	p, host := start(t, exeSpec(t, "serve"), Options{}, nil)
	if err := p.(plugin.Processor).Process(context.Background(), newProc("please notify")); err != nil {
		t.Fatal(err)
	}
	host.mu.Lock()
	defer host.mu.Unlock()
	if len(host.msgs) != 1 || host.msgs[0] != "hello from plugin" {
		t.Fatalf("%v", host.msgs)
	}
}

func TestPluginErrorsAreStableAndSessionSurvives(t *testing.T) {
	p, _ := start(t, exeSpec(t, "serve"), Options{}, nil)
	proc := p.(plugin.Processor)
	ctx := context.Background()
	for text, code := range map[string]string{"please panic": rpc.CodePanic, "fail now": rpc.CodeInternal, "badtag": rpc.CodeInvalidArgument} {
		pc := newProc(text)
		err := proc.Process(ctx, pc)
		var re *rpc.Error
		if !errors.As(err, &re) || re.Code != code {
			t.Fatalf("%q: %v", text, err)
		}
		if strings.Contains(err.Error(), "secret") || strings.Contains(err.Error(), "boom") {
			t.Fatalf("error leaks plugin text: %v", err)
		}
	}
	pc := newProc("a@b")
	if err := proc.Process(ctx, pc); err != nil || !slices.Contains(pc.policy.Tags, "email") {
		t.Fatalf("session did not survive: %v", err)
	}
}

func TestHangIsCancelledKilledAndRestarted(t *testing.T) {
	opts := Options{ProcessTimeout: 80 * time.Millisecond, CancelGrace: 100 * time.Millisecond, Backoff: []time.Duration{20 * time.Millisecond}}
	p, _ := start(t, exeSpec(t, "serve"), opts, nil)
	proc := p.(plugin.Processor)
	begin := time.Now()
	err := proc.Process(context.Background(), newProc("hang forever"))
	var re *rpc.Error
	if !errors.As(err, &re) || re.Code != rpc.CodeTimeout {
		t.Fatalf("got %v", err)
	}
	if d := time.Since(begin); d > time.Second {
		t.Fatalf("took %v", d)
	}
	waitUntil(t, "restart", func() bool {
		return proc.Process(context.Background(), newProc("a@b")) == nil
	})
}

func TestCrashRestartsThenGivesUp(t *testing.T) {
	opts := Options{Backoff: []time.Duration{10 * time.Millisecond}, MaxRestarts: 2, RestartWindow: time.Minute}
	p, _ := start(t, exeSpec(t, "serve"), opts, nil)
	proc := p.(plugin.Processor)
	crash := func() {
		waitUntil(t, "plugin available", func() bool { return proc.Process(context.Background(), newProc("fine")) == nil })
		if err := proc.Process(context.Background(), newProc("crash now")); err == nil {
			t.Fatal("crash reported success")
		}
	}
	crash() // 1st crash: restarts
	crash() // 2nd crash: restarts
	crash() // 3rd within the window: exceeds the budget
	time.Sleep(200 * time.Millisecond)
	err := proc.Process(context.Background(), newProc("fine"))
	var re *rpc.Error
	if !errors.As(err, &re) || re.Code != rpc.CodeUnavailable {
		t.Fatalf("want unavailable, got %v", err)
	}

	r := p.(interface {
		Restart(context.Context) error
		Unavailable() bool
	})
	if !r.Unavailable() {
		t.Fatal("not reported unavailable")
	}
	if err := r.Restart(context.Background()); err != nil {
		t.Fatalf("explicit restart: %v", err)
	}
	if err := proc.Process(context.Background(), newProc("a@b")); err != nil {
		t.Fatalf("not working after restart: %v", err)
	}
	if err := r.Restart(context.Background()); !errors.As(err, &re) || re.Code != rpc.CodeInvalidState {
		t.Fatalf("restarting a running plugin: %v", err)
	}
	// the crash budget starts over
	if err := proc.Process(context.Background(), newProc("crash now")); err == nil {
		t.Fatal("crash reported success")
	}
	waitUntil(t, "restarted by backoff again", func() bool { return proc.Process(context.Background(), newProc("fine")) == nil })
}

func TestStartFailures(t *testing.T) {
	ctx := context.Background()
	t.Run("digest mismatch", func(t *testing.T) {
		s := exeSpec(t, "serve")
		s.SHA256 = strings.Repeat("0", 64)
		if err := New(s, Options{}).Init(ctx, &fakeHost{}, nil); err == nil {
			t.Fatal("started a modified executable")
		}
	})
	t.Run("missing digest", func(t *testing.T) {
		s := exeSpec(t, "serve")
		s.SHA256 = ""
		if err := New(s, Options{}).Init(ctx, &fakeHost{}, nil); err == nil {
			t.Fatal("started without a digest")
		}
	})
	t.Run("garbage handshake", func(t *testing.T) {
		if err := New(exeSpec(t, "garbage"), Options{StartTimeout: time.Second}).Init(ctx, &fakeHost{}, nil); err == nil {
			t.Fatal("accepted garbage")
		}
	})
	t.Run("identity mismatch", func(t *testing.T) {
		if err := New(exeSpec(t, "other-identity"), Options{StartTimeout: time.Second}).Init(ctx, &fakeHost{}, nil); err == nil {
			t.Fatal("accepted a different plugin")
		}
	})
	t.Run("invalid config", func(t *testing.T) {
		err := New(exeSpec(t, "serve"), Options{}).Init(ctx, &fakeHost{}, plugin.Config{"bad": true})
		var re *rpc.Error
		if !errors.As(err, &re) || re.Code != rpc.CodeInvalidConfig {
			t.Fatalf("got %v", err)
		}
	})
}

func TestObserverReceivesEvents(t *testing.T) {
	p, host := start(t, exeSpec(t, "serve"), Options{}, nil)
	obs, ok := p.(plugin.Observer)
	if !ok {
		t.Fatal("the proxy hides the observer capability")
	}
	ctx := context.Background()
	if err := obs.OnEvent(ctx, plugin.Event{Kind: plugin.EventCommitted, ItemID: "h1", AtMs: 5}); err != nil {
		t.Fatal(err)
	}
	var re *rpc.Error
	if err := obs.OnEvent(ctx, plugin.Event{Kind: plugin.EventDeleted, ItemID: "fail"}); !errors.As(err, &re) || re.Code != rpc.CodeInternal {
		t.Fatalf("got %v", err)
	}
	if err := obs.OnEvent(ctx, plugin.Event{Kind: plugin.EventUpdated, ItemID: "h2"}); err != nil {
		t.Fatalf("the session must survive a failed event: %v", err)
	}
	deadline := time.Now().Add(2 * time.Second)
	for {
		host.mu.Lock()
		got := slices.Clone(host.msgs)
		host.mu.Unlock()
		if slices.Equal(got, []string{"event clipboard.committed h1", "event clipboard.updated h2"}) {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("got %q", got)
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestSubscriptionLimitsObserverEvents(t *testing.T) {
	s := exeSpec(t, "serve")
	s.Events = []string{plugin.EventCaptured, plugin.EventDeleted}
	p, _ := start(t, s, Options{}, nil)
	var re *rpc.Error
	err := p.(plugin.Observer).OnEvent(context.Background(), plugin.Event{Kind: plugin.EventCommitted, ItemID: "x"})
	if !errors.As(err, &re) || re.Code != rpc.CodeInvalidArgument {
		t.Fatalf("an unsubscribed event reached the plugin: %v", err)
	}
	if err := p.(plugin.Observer).OnEvent(context.Background(), plugin.Event{Kind: plugin.EventDeleted, ItemID: "x"}); err != nil {
		t.Fatal(err)
	}
}

func TestCloseEndsStubbornProcess(t *testing.T) {
	p, _ := start(t, exeSpec(t, "ignore-eof"), Options{StopTimeout: 400 * time.Millisecond}, nil)
	px := p.(interface{ current() *session })
	s := px.current()
	begin := time.Now()
	if err := p.Close(context.Background()); err != nil {
		t.Fatal(err)
	}
	select {
	case <-s.exited:
	default:
		t.Fatal("process still running after Close")
	}
	if d := time.Since(begin); d > 3*time.Second {
		t.Fatalf("Close took %v", d)
	}
	if err := p.Close(context.Background()); err != nil {
		t.Fatal(err) // idempotent
	}
	if err := p.(plugin.Processor).Process(context.Background(), newProc("x")); err == nil {
		t.Fatal("Process after Close succeeded")
	}
}

func waitUntil(t *testing.T, what string, ok func() bool) {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for time.Now().Before(deadline) {
		if ok() {
			return
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", what)
}

func TestStagedExecutableIsWhatWasHashed(t *testing.T) {
	src := filepath.Join(t.TempDir(), "plug")
	if err := os.WriteFile(src, []byte("good bytes"), 0o700); err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256([]byte("good bytes"))
	want := hex.EncodeToString(sum[:])

	staged, cleanup, err := stageExecutable(src, want)
	if err != nil {
		t.Fatal(err)
	}
	// Swapping the original after verification must not affect what runs.
	if err := os.WriteFile(src, []byte("evil bytes"), 0o700); err != nil {
		t.Fatal(err)
	}
	got, err := os.ReadFile(staged)
	if err != nil || string(got) != "good bytes" {
		t.Fatalf("staged copy = %q, %v", got, err)
	}
	if staged == src {
		t.Fatal("executes the original")
	}
	cleanup()
	if _, err := os.Stat(staged); !os.IsNotExist(err) {
		t.Fatalf("copy not removed: %v", err)
	}
	// A mismatch leaves nothing behind.
	if _, _, err := stageExecutable(src, want); err == nil {
		t.Fatal("accepted modified bytes")
	}
}
