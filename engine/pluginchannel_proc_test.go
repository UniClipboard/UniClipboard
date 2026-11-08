package engine

import (
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/pluginproc"
	"github.com/uniclipboard/unic/plugin"
)

// chanHelper is an external-channel backend running in a subprocess. Text
// "fail*" is reported failed; "echo*" is delivered back through the channel;
// "crash*" kills the process; anything else is reported sent.
type chanHelper struct{ rcv plugin.Receiver }

func chanHelperSpec(id string) plugin.Spec {
	return plugin.Spec{ID: id, Version: "1.0.0", APIVersion: 1, Capabilities: []string{plugin.CapSyncBackend},
		TransportKind: plugin.TransportObjectStore, ExternalChannels: true, RemoteRevocation: true}
}

func (*chanHelper) Spec() plugin.Spec                                      { return chanHelperSpec(envID()) }
func (*chanHelper) Init(context.Context, plugin.Host, plugin.Config) error { return nil }
func (*chanHelper) Start(context.Context) error                            { return nil }
func (*chanHelper) Close(context.Context) error                            { return nil }
func (c *chanHelper) SetReceiver(r plugin.Receiver)                        { c.rcv = r }
func (*chanHelper) LeaveChannel(_ context.Context, id string) error {
	if strings.HasPrefix(os.Getenv("UNIC_TEST_LEAVE"), "fail") {
		return errors.New("net down")
	}
	return nil
}
func (c *chanHelper) Submit(ctx context.Context, v plugin.SyncView, h plugin.SyncHost) (bool, error) {
	rc, err := v.Parts[0].Open(ctx)
	if err != nil {
		return false, err
	}
	b, _ := io.ReadAll(rc)
	_ = rc.Close()
	text := string(b)
	if strings.HasPrefix(text, "crash") && strings.Contains(text, envID()) {
		os.Exit(3)
	}
	if err := h.Register(v.Targets); err != nil {
		return false, err
	}
	go func() {
		for _, t := range v.Targets {
			switch {
			case strings.HasPrefix(text, "fail"):
				h.Report(t, plugin.SyncFailed, errors.New("secret"))
			case strings.HasPrefix(text, "echo"):
				_ = c.rcv.Receive(context.Background(), t, plugin.Received{Transfer: "x-" + v.ID, View: "v-" + text,
					Parts: []plugin.ReceivedPart{{MIME: MimeText, Data: []byte("reply:" + text)}}})
				h.Report(t, plugin.SyncSent, nil)
			default:
				h.Report("not-registered", plugin.SyncSent, nil)
				h.Report(t, plugin.SyncSent, nil)
			}
		}
	}()
	return true, nil
}

func envID() string {
	if v := os.Getenv("UNIC_TEST_ID"); v != "" {
		return v
	}
	return "example.wx"
}

func chanInstance() PluginInstance { return chanInstanceID("example.wx") }

func chanInstanceID(id string) PluginInstance {
	path, digest := procExe()
	return PluginInstance{Process: &PluginProcess{
		Path: path, SHA256: digest, Args: []string{"-test.run=^$"}, Env: []string{"UNIC_TEST_PLUGIN=channel", "UNIC_TEST_ID=" + id}, Spec: chanHelperSpec(id),
		Options: pluginproc.Options{ProcessTimeout: 2 * time.Second, CancelGrace: 100 * time.Millisecond, Backoff: []time.Duration{20 * time.Millisecond}},
	}}
}

func TestProcessChannelBackend(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{chanInstance()} })
	start(t, n)
	c := addCh(t, n, "example.wx")
	if !c.RemoteRevocation {
		t.Fatal("remote revocation must surface")
	}
	send := func(id, text string) SubmissionResult { return commitText(t, n.e, id, text) }

	r := send("s1", "hello")
	if r.SyncDisposition != SyncQueued {
		t.Fatalf("%+v", r)
	}
	eventually(t, "sent", func() bool {
		op, err := n.e.GetOperation(bg, r.OperationID)
		return err == nil && op.Phase == PhaseCompleted && len(op.Targets) == 1 && op.Targets[0].TargetID == c.ID
	})

	r = send("s2", "fail please")
	eventually(t, "failed", func() bool {
		op, err := n.e.GetOperation(bg, r.OperationID)
		if err != nil || len(op.Targets) != 1 {
			return false
		}
		if e := op.Targets[0].Error; e != nil && strings.Contains(e.Message, "secret") {
			t.Fatal("plugin text leaked")
		}
		return op.Targets[0].State == TargetFailed
	})

	send("s3", "echo me")
	rs := waitReceived(t, n.e, 1)
	if rs[0].ChannelID != c.ID || rs[0].PeerID != "" {
		t.Fatalf("%+v", rs)
	}
	page, err := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{OriginKinds: []HistoryOriginKind{OriginKindExternal}}})
	if err != nil || len(page.Items) != 1 || page.Items[0].Origin.PluginID != "example.wx" {
		t.Fatalf("%+v %v", page, err)
	}

	// a crash mid-submit fails that send; the backend restarts and works again
	send("s4", "crash example.wx")
	eventually(t, "recovered", func() bool {
		r := send("ok"+time.Now().String(), "hello again")
		return r.SyncDisposition == SyncQueued
	})

	if err := n.e.RemoveChannel(bg, RemoveChannelRequest{ID: c.ID, ExpectedRevision: mustChannel(t, n, c.ID).Revision}); err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.GetChannel(bg, c.ID); err == nil {
		t.Fatal("channel must be gone after a successful remote leave")
	}
}

func mustChannel(t *testing.T, n node, id string) Channel {
	t.Helper()
	c, err := n.e.GetChannel(bg, id)
	if err != nil {
		t.Fatal(err)
	}
	return c
}

func TestBackendInstanceDirAndLock(t *testing.T) {
	dir := t.TempDir()
	opts, _ := newOpts(t, dir)
	opts.Plugins = []PluginInstance{chanInstance(), {Plugin: &storeBackend{id: "test.repo", kind: plugin.TransportObjectStore}}}
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{"example.wx", "test.repo"} {
		if _, _, err := openInstanceDir(dir, id); !errors.Is(err, errLocked) {
			t.Fatalf("%s: a running backend must hold its instance lock, got %v", id, err)
		}
	}
	if err := e.Close(bg); err != nil {
		t.Fatal(err)
	}
	for _, id := range []string{"example.wx", "test.repo"} {
		_, l, err := openInstanceDir(dir, id)
		if err != nil {
			t.Fatalf("%s: lock must be released on close: %v", id, err)
		}
		l.release()
	}

	own := chanInstance()
	own.Process.Dir = t.TempDir()
	opts.Plugins = []PluginInstance{own}
	if e, err := New(bg, opts); err == nil {
		_ = e.Close(bg)
		t.Fatal("a backend must not choose its own directory")
	}
}

// Two instances of one implementation share nothing: channels, targets,
// directories, crashes and history origins stay per instance.
func TestTwoChannelBackendInstancesAreIsolated(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.Plugins = []PluginInstance{chanInstanceID("example.wx"), chanInstanceID("example.wy")}
	})
	start(t, n)
	cx, cy := addCh(t, n, "example.wx"), addCh(t, n, "example.wy")
	if cx.ID == cy.ID {
		t.Fatal("channels of different instances must differ")
	}
	r := commitText(t, n.e, "both", "hello")
	eventually(t, "both sent", func() bool {
		op, err := n.e.GetOperation(bg, r.OperationID)
		if err != nil || op.Phase != PhaseCompleted || len(op.Targets) != 2 {
			return false
		}
		got := map[string]PluginID{}
		for _, tg := range op.Targets {
			got[tg.TargetID] = tg.BackendID
		}
		return got[cx.ID] == "example.wx" && got[cy.ID] == "example.wy"
	})

	// disabling one channel leaves the other instance's sending untouched
	if _, err := n.e.SetChannelEnabled(bg, SetChannelEnabledRequest{ID: cx.ID, ExpectedRevision: cx.Revision, Enabled: false}); err != nil {
		t.Fatal(err)
	}
	r = commitText(t, n.e, "one", "hello 2")
	op, _ := n.e.GetOperation(bg, r.OperationID)
	if len(op.Targets) != 1 || op.Targets[0].BackendID != "example.wy" {
		t.Fatalf("%+v", op.Targets)
	}

	// content received through one instance is attributed to it only
	commitText(t, n.e, "echo", "echo me")
	waitReceived(t, n.e, 1)
	page, _ := n.e.ListHistory(bg, ListHistoryRequest{Filter: HistoryFilter{OriginKinds: []HistoryOriginKind{OriginKindExternal}}})
	if len(page.Items) != 1 || page.Items[0].Origin.PluginID != "example.wy" {
		t.Fatalf("%+v", page.Items)
	}

	// one backend crashing does not break the other
	cx, err := n.e.GetChannel(bg, cx.ID)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := n.e.SetChannelEnabled(bg, SetChannelEnabledRequest{ID: cx.ID, ExpectedRevision: cx.Revision, Enabled: true}); err != nil {
		t.Fatal(err)
	}
	r = commitText(t, n.e, "crash", "crash example.wy")
	eventually(t, "other instance keeps working", func() bool {
		op, err := n.e.GetOperation(bg, r.OperationID)
		if err != nil {
			return false
		}
		for _, tg := range op.Targets {
			if tg.BackendID == "example.wx" && tg.State == TargetSent {
				return true
			}
		}
		return false
	})
	eventually(t, "crashed instance recovers", func() bool {
		r := commitText(t, n.e, "again"+time.Now().String(), "back")
		op, err := n.e.GetOperation(bg, r.OperationID)
		return err == nil && len(op.Targets) == 2
	})

	for _, id := range []string{"example.wx", "example.wy"} {
		if _, err := os.Stat(filepath.Join(n.dir, "plugin-data", id)); err != nil {
			t.Fatalf("%s: %v", id, err)
		}
	}
}
