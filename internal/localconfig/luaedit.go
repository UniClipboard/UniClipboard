package config

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"

	"github.com/rogpeppe/go-internal/lockedfile"
)

var tableHeader = regexp.MustCompile(`^\s*\[\[?\s*([^\[\]]+?)\s*\]\]?\s*(#.*)?$`)
var luaKey = regexp.MustCompile(`^\s*lua\s*=`)
var orderKey = regexp.MustCompile(`^\s*order\s*=`)
var enabledKey = regexp.MustCompile(`^\s*enabled\s*=`)

// SetLuaEnabled adds id to, or removes it from, plugins.lua in the file at
// path, leaving everything else (comments, other settings) as it was. Enabling
// appends at the end of the list. It reports whether the list changed. The
// edited text is parsed again before it replaces the file, so a bad edit can
// never leave a configuration the daemon refuses.
func SetLuaEnabled(path, id string, on bool) (changed bool, err error) {
	return setPluginEnabled(path, id, on, "lua")
}

func SetOrderedPluginEnabled(path, id string, on bool) (bool, error) {
	return setPluginEnabled(path, id, on, "order")
}

// DisableExternalPlugin removes an ID from both external plugin lists in one save.
func DisableExternalPlugin(path, id string) (bool, error) {
	return setPluginEnabled(path, id, false, "lua", "order")
}

func configWriteLock(path string) (func(), error) {
	return lockedfile.MutexAt(filepath.Join(filepath.Dir(path), ".config-write.lock")).Lock()
}

func setPluginEnabled(path, id string, on bool, keys ...string) (bool, error) {
	unlock, err := configWriteLock(path)
	if err != nil {
		return false, err
	}
	defer unlock()
	data, err := os.ReadFile(path)
	if err != nil {
		return false, err
	}
	cur, err := Parse(data)
	if err != nil {
		return false, fmt.Errorf("%s is not valid, fix it first: %w", filepath.Base(path), err)
	}
	out := string(data)
	lists := map[string][]string{"lua": cur.Plugins.Lua, "order": cur.Plugins.Order}
	for _, key := range keys {
		list := slices.Clone(lists[key])
		has := slices.Contains(list, id)
		switch {
		case on && !has:
			list = append(list, id)
		case !on && has:
			list = slices.DeleteFunc(list, func(s string) bool { return s == id })
		default:
			continue
		}
		lists[key] = list
		out = rewritePluginList(out, list, key)
	}
	if in, ok := cur.Plugins.Instances[id]; ok && !strings.HasPrefix(id, "builtin.") {
		active := slices.Contains(lists["lua"], id) || slices.Contains(lists["order"], id)
		if active != (in.Enabled == nil) {
			out = rewriteExternalEnabled(out, id, active)
		}
	}
	if out == string(data) {
		return false, nil
	}
	next, err := Parse([]byte(out))
	if err != nil || !slices.Equal(next.Plugins.Lua, lists["lua"]) || !slices.Equal(next.Plugins.Order, lists["order"]) {
		return false, fmt.Errorf("cannot edit external plugin lists automatically; edit %s by hand", filepath.Base(path))
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".config-*")
	if err != nil {
		return false, err
	}
	defer func() { _ = os.Remove(tmp.Name()) }()
	if _, err = tmp.WriteString(out); err == nil {
		err = tmp.Sync()
	}
	if cerr := tmp.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		return false, err
	}
	return true, os.Rename(tmp.Name(), path)
}

func rewriteExternalEnabled(text, id string, active bool) string {
	header := fmt.Sprintf("plugins.instances.%q", id)
	singleQuoted := fmt.Sprintf("plugins.instances.'%s'", id)
	lines := strings.Split(text, "\n")
	start := -1
	for i, line := range lines {
		if m := tableHeader.FindStringSubmatch(line); m != nil {
			if start >= 0 {
				break
			}
			if m[1] == header || m[1] == singleQuoted {
				start = i
			}
			continue
		}
		if start >= 0 && enabledKey.MatchString(line) {
			if active {
				return strings.Join(slices.Delete(lines, i, i+1), "\n")
			}
			lines[i] = "enabled = false"
			return strings.Join(lines, "\n")
		}
	}
	if active {
		return text
	}
	if start >= 0 {
		return strings.Join(slices.Insert(lines, start+1, "enabled = false"), "\n")
	}
	return strings.TrimRight(text, "\n") + "\n\n[" + header + "]\nenabled = false\n"
}

func pluginLine(list []string, key string) string {
	q := make([]string, len(list))
	for i, s := range list {
		q[i] = fmt.Sprintf("%q", s)
	}
	return key + " = [" + strings.Join(q, ", ") + "]"
}

// rewritePluginList replaces or appends the named list in [plugins].
func rewritePluginList(text string, list []string, key string) string {
	matchKey := luaKey
	if key == "order" {
		matchKey = orderKey
	}
	lines := strings.Split(text, "\n")
	table, hdr := "", -1
	for i := 0; i < len(lines); i++ {
		if m := tableHeader.FindStringSubmatch(lines[i]); m != nil && !strings.HasPrefix(strings.TrimSpace(lines[i]), "#") {
			table = m[1]
			if table == "plugins" {
				hdr = i
			}
			continue
		}
		if table != "plugins" || !matchKey.MatchString(lines[i]) {
			continue
		}
		end := i
		for depth := bracketDepth(lines[i]); depth > 0 && end+1 < len(lines); {
			end++
			depth += bracketDepth(lines[end])
		}
		return strings.Join(append(append(slices.Clone(lines[:i]), pluginLine(list, key)), lines[end+1:]...), "\n")
	}
	if hdr >= 0 {
		return strings.Join(append(append(slices.Clone(lines[:hdr+1]), pluginLine(list, key)), lines[hdr+1:]...), "\n")
	}
	return strings.TrimRight(text, "\n") + "\n\n[plugins]\n" + pluginLine(list, key) + "\n"
}

// bracketDepth counts [ minus ] outside strings and comments.
func bracketDepth(s string) int {
	d, q := 0, false
	for _, r := range s {
		switch {
		case r == '"':
			q = !q
		case q:
		case r == '#':
			return d
		case r == '[':
			d++
		case r == ']':
			d--
		}
	}
	return d
}
