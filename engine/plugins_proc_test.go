package engine

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/plugin"
)

// procHelper is the plugin side of the process tests: the test binary
// re-executes itself with UNIC_TEST_PLUGIN set.
type procHelper struct{}

func procHelperSpec() plugin.Spec {
	return plugin.Spec{ID: "example.helper", Version: "1.0.0", APIVersion: 1,
		Capabilities: []string{plugin.CapProcessor, plugin.CapAction},
		Actions:      []plugin.ActionSpec{{ID: "upper", Title: "Upper", Accepts: []string{"text"}}}}
}

func (procHelper) Spec() plugin.Spec                                      { return procHelperSpec() }
func (procHelper) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (procHelper) Start(context.Context) error                            { return nil }
func (procHelper) Close(context.Context) error                            { return nil }
func (procHelper) Process(_ context.Context, pc plugin.ProcessContext) error {
	text := pc.Item().Text
	if strings.Contains(text, "crash") {
		os.Exit(3)
	}
	if strings.Contains(text, "hang") {
		time.Sleep(time.Hour)
	}
	if strings.Contains(text, "@") {
		return pc.AddTag("email")
	}
	return nil
}
func (procHelper) Run(_ context.Context, ac plugin.ActionContext) error {
	return ac.SetText(strings.ToUpper(ac.Item().Text))
}

func TestMain(m *testing.M) {
	if v := os.Getenv("UNIC_TEST_PLUGIN"); v != "" {
		if v == "tcppeer" {
			_ = plugin.Serve(&tcpPeer{})
			os.Exit(0)
		}
		if v == "channel" {
			_ = plugin.Serve(&chanHelper{})
			os.Exit(0)
		}
		_ = plugin.Serve(procHelper{})
		os.Exit(0)
	}
	os.Exit(m.Run())
}

var procExe = sync.OnceValues(func() (string, string) {
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

func procInstance(onError string) PluginInstance {
	path, digest := procExe()
	return PluginInstance{OnError: onError, Process: &PluginProcess{
		Path: path, SHA256: digest, Args: []string{"-test.run=^$"}, Env: []string{"UNIC_TEST_PLUGIN=1"}, Spec: procHelperSpec(),
		Options: pluginproc.Options{ProcessTimeout: 150 * time.Millisecond, CancelGrace: 100 * time.Millisecond, Backoff: []time.Duration{20 * time.Millisecond}},
	}}
}

func TestProcessPluginAnnotatesAndRunsActions(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{procInstance("")} })
	start(t, n)
	r := commitText(t, n.e, "m", "mail me@example.com")
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil || !slices.Equal(it.Tags, []string{"email"}) {
		t.Fatalf("%v %v", it.Tags, err)
	}
	a, err := n.e.RunAction(bg, RunActionRequest{RequestID: "a", Key: "example.helper/upper", HistoryID: r.HistoryID})
	if err != nil {
		t.Fatal(err)
	}
	if got := historyText(t, n.e, a.HistoryID); got != "MAIL ME@EXAMPLE.COM" {
		t.Fatalf("got %q", got)
	}
	ai, _ := n.e.GetItem(bg, a.HistoryID)
	if ai.Origin.Kind != OriginKindAction || ai.Origin.PluginID != "example.helper" || !slices.Equal(ai.Tags, []string{"email"}) {
		t.Fatalf("%+v", ai)
	}
}

func TestHungProcessPluginDoesNotBlockCommit(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{procInstance("")} })
	start(t, n)
	begin := time.Now()
	r := commitText(t, n.e, "h", "hang forever me@example.com")
	if d := time.Since(begin); d > 3*time.Second {
		t.Fatalf("commit took %v", d)
	}
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil || len(it.Tags) != 0 {
		t.Fatalf("a failed processor must leave no annotations: %v %v", it.Tags, err)
	}
	// the plugin is restarted and works again
	deadline := time.Now().Add(10 * time.Second)
	for {
		r = commitText(t, n.e, "ok"+time.Now().String(), "mail me@example.com")
		if it, _ := n.e.GetItem(bg, r.HistoryID); slices.Contains(it.Tags, "email") {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("plugin never recovered")
		}
		time.Sleep(50 * time.Millisecond)
	}
}

func TestProcessPluginRejectedAtStartup(t *testing.T) {
	bad := procInstance("")
	bad.Process.SHA256 = strings.Repeat("0", 64)
	opts, _ := newOpts(t, t.TempDir())
	opts.Plugins = []PluginInstance{bad}
	if e, err := New(bg, opts); err == nil {
		_ = e.Close(bg)
		t.Fatal("started with a wrong digest")
	}
	both := procInstance("")
	both.Plugin = procHelper{}
	opts.Plugins = []PluginInstance{both}
	if e, err := New(bg, opts); err == nil {
		_ = e.Close(bg)
		t.Fatal("accepted Plugin and Process together")
	}
	opts.Plugins = []PluginInstance{procInstance("explode")}
	if e, err := New(bg, opts); err == nil {
		_ = e.Close(bg)
		t.Fatal("accepted an unknown on_error")
	}
}

func TestRestartPluginAfterItGaveUp(t *testing.T) {
	in := procInstance("")
	in.Process.Options.MaxRestarts = 1
	in.Process.Options.RestartWindow = time.Minute
	n := newContentNode(t, func(o *Options) {
		o.Plugins = []PluginInstance{in, {Plugin: fakeProcessor{fakePlugin{spec: plugin.Spec{ID: "ok.inproc", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}, closed: new([]string)}}}}
	})
	start(t, n)
	id := PluginID("example.helper")
	if code(n.e.RestartPlugin(bg, id)) != CodeInvalidState {
		t.Fatal("restarted a running plugin")
	}
	if code(n.e.RestartPlugin(bg, "no.such")) != CodeNotFound {
		t.Fatal("restarted an unknown plugin")
	}
	if code(n.e.RestartPlugin(bg, "ok.inproc")) != CodeUnsupported {
		t.Fatal("restarted an in-process plugin")
	}
	tagged := func(req string) bool {
		r := commitText(t, n.e, req, "mail "+req+"@example.com") // unique: identical content is deduplicated
		it, _ := n.e.GetItem(bg, r.HistoryID)
		return slices.Contains(it.Tags, "email")
	}
	for i := 0; i < 2; i++ { // a budget of one restart is exceeded by the second crash
		eventually(t, "plugin working", func() bool { return tagged(fmt.Sprintf("w%d-%d", i, time.Now().UnixNano())) })
		commitText(t, n.e, fmt.Sprintf("c%d", i), fmt.Sprintf("crash now %d", i))
	}
	eventually(t, "the plugin gave up and restarts on request", func() bool { return n.e.RestartPlugin(bg, id) == nil })
	if !tagged("after") {
		t.Fatal("not working after an explicit restart")
	}
}
