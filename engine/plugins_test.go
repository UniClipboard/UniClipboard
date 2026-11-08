package engine

import (
	"context"
	"errors"
	"slices"
	"testing"

	"github.com/uniclipboard/unic/internal/plugins/builtintext"
	"github.com/uniclipboard/unic/plugin"
)

func TestBuiltinPluginTagsAndRunsActions(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.Plugins = []PluginInstance{{Plugin: builtintext.New()}} })
	start(t, n)
	r := commitText(t, n.e, "m", "mail me@example.com or see https://example.com")
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil || !slices.Equal(it.Tags, []string{"email", "url"}) {
		t.Fatalf("%v %v", it.Tags, err)
	}
	list, err := n.e.ListActions(bg)
	if err != nil || len(list) != 4 || list[0].Key != "builtin.text/lower" {
		t.Fatalf("%+v %v", list, err)
	}
	a, err := n.e.RunAction(bg, RunActionRequest{RequestID: "a", Key: "builtin.text/upper", HistoryID: r.HistoryID})
	if err != nil {
		t.Fatal(err)
	}
	if historyText(t, n.e, a.HistoryID) != "MAIL ME@EXAMPLE.COM OR SEE HTTPS://EXAMPLE.COM" {
		t.Fatal("wrong result")
	}
	// the result is annotated like any other new item
	got, _ := n.e.GetItem(bg, a.HistoryID)
	if !slices.Equal(got.Tags, []string{"email", "url"}) || got.Origin.Kind != OriginKindAction || got.Origin.PluginID != builtintext.ID {
		t.Fatalf("%+v", got)
	}
}

type fakePlugin struct {
	spec    plugin.Spec
	initErr error
	closed  *[]string
}

func (f fakePlugin) Spec() plugin.Spec                                      { return f.spec }
func (f fakePlugin) Init(context.Context, plugin.Host, plugin.Config) error { return f.initErr }
func (f fakePlugin) Start(context.Context) error                            { return nil }
func (f fakePlugin) Close(context.Context) error {
	*f.closed = append(*f.closed, f.spec.ID)
	return nil
}

type fakeProcessor struct{ fakePlugin }

func (fakeProcessor) Process(context.Context, plugin.ProcessContext) error { return nil }

func TestPluginsAreValidatedAndUnwoundOnFailure(t *testing.T) {
	good := func(id string, closed *[]string) PluginInstance {
		return PluginInstance{Plugin: fakeProcessor{fakePlugin{spec: plugin.Spec{ID: id, Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}, closed: closed}}}
	}
	var closed []string
	bad := map[string]PluginInstance{
		"reserved":   good("sync.fake", &closed),
		"bad id":     good("Fake", &closed),
		"api":        {Plugin: fakeProcessor{fakePlugin{spec: plugin.Spec{ID: "x.y", Version: "1", APIVersion: 2, Capabilities: []string{plugin.CapProcessor}}, closed: &closed}}},
		"no impl":    {Plugin: fakePlugin{spec: plugin.Spec{ID: "x.y", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}, closed: &closed}},
		"capability": {Plugin: fakeProcessor{fakePlugin{spec: plugin.Spec{ID: "x.y", Version: "1", APIVersion: 1, Capabilities: []string{"observer"}}, closed: &closed}}},
		"init":       {Plugin: fakeProcessor{fakePlugin{spec: plugin.Spec{ID: "x.y", Version: "1", APIVersion: 1, Capabilities: []string{plugin.CapProcessor}}, initErr: errors.New("secret body"), closed: &closed}}},
		"event":      {Plugin: good("x.y", &closed).Plugin, Events: []string{"clipboard.pasted"}},
	}
	for name, in := range bad {
		closed = nil
		opts, _ := newOpts(t, t.TempDir())
		opts.Plugins = []PluginInstance{good("ok.first", &closed), in}
		e, err := New(bg, opts)
		if err == nil {
			_ = e.Close(bg)
			t.Errorf("%s: accepted", name)
			continue
		}
		if code(err) != CodeInvalidArgument || !slices.Equal(closed, []string{"ok.first"}) {
			t.Errorf("%s: %v closed=%v", name, err, closed)
		}
		if name == "init" && errors.Is(err, errors.New("secret body")) {
			t.Error("leaked")
		}
	}
	closed = nil
	opts, _ := newOpts(t, t.TempDir())
	opts.Plugins = []PluginInstance{good("ok.a", &closed), good("ok.a", &closed)}
	if _, err := New(bg, opts); code(err) != CodeInvalidArgument {
		t.Fatalf("duplicate: %v", err)
	}
	opts, _ = newOpts(t, t.TempDir())
	opts.Plugins = []PluginInstance{good("ok.a", &closed), good("ok.b", &closed)}
	closed = nil
	e, err := New(bg, opts)
	if err != nil {
		t.Fatal(err)
	}
	_ = e.Close(bg)
	if !slices.Equal(closed, []string{"ok.b", "ok.a"}) {
		t.Fatalf("close order: %v", closed)
	}
}
