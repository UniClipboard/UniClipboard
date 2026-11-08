package engine

import (
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

const luaTagger = `
local tag
return {
  id = "lua.tagger", version = "1.0.0", api_version = 1,
  capabilities = {"processor", "action"},
  actions = {{id = "shout", title = "Shout", accepts = {"text"}}},
  init = function(host, config)
    for k in pairs(config) do if k ~= "tag" then error("unknown key") end end
    tag = config.tag or "lua"
  end,
  process = function(ctx)
    if ctx.item.type == "text" and ctx.item.text:find("@", 1, true) then ctx:add_tag(tag) end
    if ctx.item.text:find("secret", 1, true) then ctx:set_sync(false) end
  end,
  run_action = function(ctx)
    ctx:set_text(ctx.item.text:upper() .. "!")
  end,
}`

func luaSpec(src string) LuaPluginSpec {
	return LuaPluginSpec{ID: "lua.tagger", Version: "1.0.0", APIVersion: 1, Capabilities: []string{"processor", "action"}, Source: src, Config: plugin.Config{"tag": "mail"}}
}

func TestLuaPluginProcessesAndActs(t *testing.T) {
	n := newContentNode(t, func(o *Options) { o.LuaPlugins = []LuaPluginSpec{luaSpec(luaTagger)} })
	start(t, n)
	r := commitText(t, n.e, "m", "me@example.com")
	it, err := n.e.GetItem(bg, r.HistoryID)
	if err != nil || !slices.Equal(it.Tags, []string{"mail"}) {
		t.Fatalf("%v %v", it.Tags, err)
	}
	a, err := n.e.RunAction(bg, RunActionRequest{RequestID: "a", Key: "lua.tagger/shout", HistoryID: r.HistoryID})
	if err != nil || historyText(t, n.e, a.HistoryID) != "ME@EXAMPLE.COM!" {
		t.Fatalf("%v", err)
	}
}

func TestLuaPluginRejectsBadDeclarations(t *testing.T) {
	for name, mut := range map[string]func(*LuaPluginSpec){
		"id mismatch":  func(s *LuaPluginSpec) { s.ID = "lua.other" },
		"caps":         func(s *LuaPluginSpec) { s.Capabilities = []string{"processor"} },
		"backend":      func(s *LuaPluginSpec) { s.Capabilities = []string{"sync_backend"} },
		"syntax":       func(s *LuaPluginSpec) { s.Source = "return {" },
		"huge":         func(s *LuaPluginSpec) { s.Source = strings.Repeat("-", 300<<10) },
		"strict":       func(s *LuaPluginSpec) { s.Config = plugin.Config{"bogus": 1} },
		"os access":    func(s *LuaPluginSpec) { s.Source = strings.Replace(luaTagger, "local tag", "local tag = os.time()", 1) },
		"loop at load": func(s *LuaPluginSpec) { s.Source = "while true do end" },
	} {
		s := luaSpec(luaTagger)
		mut(&s)
		opts, _ := newOpts(t, t.TempDir())
		opts.LuaPlugins = []LuaPluginSpec{s}
		if e, err := New(bg, opts); err == nil {
			_ = e.Close(bg)
			t.Errorf("%s: accepted", name)
		}
	}
}

func TestLuaFailureIsContainedAndReloadRecovers(t *testing.T) {
	bad := strings.Replace(luaTagger, `if ctx.item.type == "text"`, `if ctx.item.text:find("boom", 1, true) then while true do end end
    if ctx.item.type == "text"`, 1)
	s := luaSpec(bad)
	s.OnError = "block_sync"
	n := newContentNode(t, func(o *Options) { o.LuaPlugins = []LuaPluginSpec{s} })
	start(t, n)
	r := commitText(t, n.e, "boom", "boom me@x.io")
	it, _ := n.e.GetItem(bg, r.HistoryID)
	if len(it.Tags) != 0 {
		t.Fatalf("failed callback must drop its patch: %v", it.Tags)
	}
	// the plugin is now unavailable: later items are unannotated too
	r2 := commitText(t, n.e, "ok", "me@example.com")
	if it2, _ := n.e.GetItem(bg, r2.HistoryID); len(it2.Tags) != 0 {
		t.Fatalf("dead plugin ran: %v", it2.Tags)
	}
	// a broken replacement is refused; a good one restores service
	if err := n.e.ReloadLuaPlugin(bg, "lua.tagger", "return {"); err == nil {
		t.Fatal("bad reload accepted")
	}
	if err := n.e.ReloadLuaPlugin(bg, "lua.tagger", luaTagger); err != nil {
		t.Fatal(err)
	}
	r3 := commitText(t, n.e, "ok2", "you@example.com")
	if it3, _ := n.e.GetItem(bg, r3.HistoryID); !slices.Equal(it3.Tags, []string{"mail"}) {
		t.Fatalf("reload did not restore: %v", it3.Tags)
	}
	// reload of a non-Lua plugin / unknown ID
	if err := n.e.ReloadLuaPlugin(bg, "nope.x", ""); err == nil {
		t.Fatal("unknown plugin")
	}
}

func loadExample(t *testing.T, dir, id string, caps []string, cfg plugin.Config) LuaPluginSpec {
	t.Helper()
	b, err := os.ReadFile(filepath.Join("..", "plugins", "examples", "lua", dir, "main.lua"))
	if err != nil {
		t.Fatal(err)
	}
	return LuaPluginSpec{ID: id, Version: "1.0.0", APIVersion: 1, Capabilities: caps, Source: string(b), Config: cfg}
}

func TestLuaExamples(t *testing.T) {
	n := newContentNode(t, func(o *Options) {
		o.LuaPlugins = []LuaPluginSpec{
			loadExample(t, "emailtag", "example.lua-emailtag", []string{"processor"}, nil),
			loadExample(t, "sensitive", "example.lua-sensitive", []string{"processor"}, nil),
			loadExample(t, "urltag", "example.lua-urltag", []string{"processor"}, nil),
			loadExample(t, "textcase", "example.lua-textcase", []string{"action"}, nil),
		}
	})
	start(t, n)
	r := commitText(t, n.e, "a", "me@example.com")
	if it, _ := n.e.GetItem(bg, r.HistoryID); !slices.Equal(it.Tags, []string{"email"}) {
		t.Fatalf("%v", it.Tags)
	}
	r = commitText(t, n.e, "u", "https://Example.com/a?b=1")
	if it, _ := n.e.GetItem(bg, r.HistoryID); !slices.Equal(it.Tags, []string{"host:example.com", "url"}) {
		t.Fatalf("%v", it.Tags)
	}
	r = commitText(t, n.e, "u2", "see https://example.com")
	if it, _ := n.e.GetItem(bg, r.HistoryID); len(it.Tags) != 0 {
		t.Fatalf("%v", it.Tags)
	}
	r = commitText(t, n.e, "b", "password: hunter2")
	it, _ := n.e.GetItem(bg, r.HistoryID)
	if !slices.Equal(it.Tags, []string{"sensitive"}) || it.ExpiresAtMs == 0 {
		t.Fatalf("%+v", it)
	}
	a, err := n.e.RunAction(bg, RunActionRequest{RequestID: "t", Key: "example.lua-textcase/title", HistoryID: r.HistoryID})
	if err != nil || historyText(t, n.e, a.HistoryID) != "Password: Hunter2" {
		t.Fatalf("%v", err)
	}
}
