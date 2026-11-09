package host

import (
	"context"
	"time"

	"github.com/fsnotify/fsnotify"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	"github.com/uniclipboard/unic/engine"
)

const luaDebounce = 150 * time.Millisecond

// watchLua applies edits to the scripts of running Lua plugins. A changed
// script text is swapped in at a callback boundary; if it fails to load the
// old one keeps running. A change to anything the engine fixed at startup
// (identity, capabilities, events, config) or a removed script waits for a
// restart. Logs name the plugin ID and nothing from the script.
func (n *Node) watchLua(ctx context.Context) {
	w, err := fsnotify.NewWatcher()
	if err != nil {
		n.logf("lua: not watching scripts, edits need a restart")
		return
	}
	defer func() { _ = w.Close() }()
	// Directories are watched, not files: editors save by renaming over.
	watch := func() {
		_ = w.Add(n.luaDir)
		for _, s := range n.lua {
			_ = w.Add(s.Dir)
		}
	}
	watch()
	var timer *time.Timer
	var fire <-chan time.Time
	for {
		select {
		case <-ctx.Done():
			return
		case _, ok := <-w.Events:
			if !ok {
				return
			}
			if timer == nil {
				timer = time.NewTimer(luaDebounce)
			} else {
				if !timer.Stop() {
					select {
					case <-timer.C:
					default:
					}
				}
				timer.Reset(luaDebounce)
			}
			fire = timer.C
		case <-fire:
			fire = nil
			n.rescanLua(ctx)
			watch()
		case err, ok := <-w.Errors:
			if !ok {
				return
			}
			n.logf("lua: watch: %v", err)
		}
	}
}

// rescanLua compares the installed scripts with the running ones.
func (n *Node) rescanLua(ctx context.Context) {
	found, _ := luascripts.Scan(n.luaDir)
	for id, old := range n.lua {
		now, ok := found[id]
		switch {
		case !ok:
			// Missing or unreadable (possibly mid-save): keep running what we have.
			n.logf("lua: %s: script unavailable, keeping the running one", id)
		case !old.Same(now):
			n.cfg.MarkRestart("plugins.lua")
			n.logf("lua: %s: manifest changed, restart to apply", id)
		case old.Source != now.Source:
			if err := n.e.ReloadLuaPlugin(ctx, engine.PluginID(id), now.Source); err != nil {
				n.logf("lua: %s: reload failed (%s), keeping the running one", id, asCode(err))
				continue
			}
			n.lua[id] = now
			n.logf("lua: %s: reloaded", id)
		}
	}
}
