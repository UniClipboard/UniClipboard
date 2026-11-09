package host

import (
	"context"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/uniclipboard/unic/app/desktop/internal/config"
)

const manifest = `manifest_version = 1
kind = "lua"
entry = "main.lua"
id = "t.tagger"
version = "1.0.0"
api_version = 1
capabilities = ["processor"]
`

func script(tag string) string {
	return `return { id="t.tagger", version="1.0.0", api_version=1, capabilities={"processor"},
  process=function(ctx) ctx:add_tag("` + tag + `") end }`
}

func TestLuaScriptsLoadAndHotReload(t *testing.T) {
	dir := t.TempDir()
	pdir := filepath.Join(dir, "plugins", "lua", "tagger")
	if err := os.MkdirAll(pdir, 0o700); err != nil {
		t.Fatal(err)
	}
	write := func(name, s string) {
		t.Helper()
		tmp := filepath.Join(pdir, name+".tmp")
		if err := os.WriteFile(tmp, []byte(s), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Rename(tmp, filepath.Join(pdir, name)); err != nil { // atomic, like an editor
			t.Fatal(err)
		}
	}
	write("spec.toml", manifest)
	write("main.lua", script("one"))
	cfgPath := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(cfgPath, []byte("[plugins]\nlua = [\"t.tagger\"]\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	m, err := config.Open(cfgPath, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	b := &fakeBoard{}
	n, err := Open(context.Background(), Options{Dir: dir, Settings: m, Board: b, Poll: 10 * time.Millisecond, Logf: t.Logf,
		Transport: newLoopback(t), LuaDir: filepath.Join(dir, "plugins", "lua")})
	if err != nil {
		t.Fatal(err)
	}
	if err := n.Start(context.Background()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = n.Close(context.Background()) })
	h := &host{n: n, board: b, cfg: m}
	settle(h)

	// A new copy is made on each attempt until the script in effect tags it:
	// a reload takes a moment to follow the file write.
	count := 0
	tagged := func(s, tag string) {
		t.Helper()
		eventually(t, "tag "+tag, func() bool {
			count++
			b.user(text(s + strings.Repeat("+", count)))
			time.Sleep(150 * time.Millisecond)
			for _, x := range historyIDs(t, h) {
				if slices.Contains(x.Tags, tag) {
					return true
				}
			}
			return false
		})
	}
	tagged("first", "one")

	write("main.lua", script("two"))
	tagged("second", "two")

	// A broken edit keeps the running script.
	write("main.lua", "this is not lua (")
	time.Sleep(500 * time.Millisecond)
	before := len(historyIDs(t, h))
	b.user(text("third"))
	eventually(t, "kept", func() bool {
		xs := historyIDs(t, h)
		return len(xs) == before+1 && slices.Contains(xs[0].Tags, "two") && !slices.Contains(xs[0].Tags, "one")
	})

	// A manifest change cannot be applied live: it is reported.
	write("spec.toml", strings.Replace(manifest, "1.0.0", "1.1.0", 1))
	eventually(t, "restart noted", func() bool { return slices.Contains(m.State().RestartRequired, "plugins.lua") })
}

func TestLuaEnabledButMissingFailsOpen(t *testing.T) {
	dir := t.TempDir()
	cfgPath := filepath.Join(dir, "config.toml")
	if err := os.WriteFile(cfgPath, []byte("[plugins]\nlua = [\"nope\"]\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	m, err := config.Open(cfgPath, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	_, err = Open(context.Background(), Options{Dir: dir, Settings: m, Board: &fakeBoard{}, Logf: t.Logf,
		Transport: newLoopback(t), LuaDir: filepath.Join(dir, "plugins", "lua")})
	if err == nil {
		t.Fatal("a missing enabled plugin must stop startup")
	}
}
