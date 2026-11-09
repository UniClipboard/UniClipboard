package config

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
)

var tableHeader = regexp.MustCompile(`^\s*\[\[?\s*([^\[\]]+?)\s*\]\]?\s*(#.*)?$`)
var luaKey = regexp.MustCompile(`^\s*lua\s*=`)

// SetLuaEnabled adds id to, or removes it from, plugins.lua in the file at
// path, leaving everything else (comments, other settings) as it was. Enabling
// appends at the end of the list. It reports whether the list changed. The
// edited text is parsed again before it replaces the file, so a bad edit can
// never leave a configuration the daemon refuses.
func SetLuaEnabled(path, id string, on bool) (changed bool, err error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return false, err
	}
	cur, err := Parse(data)
	if err != nil {
		return false, fmt.Errorf("%s is not valid, fix it first: %w", filepath.Base(path), err)
	}
	list := slices.Clone(cur.Plugins.Lua)
	has := slices.Contains(list, id)
	switch {
	case on && has, !on && !has:
		return false, nil
	case on:
		list = append(list, id)
	default:
		list = slices.DeleteFunc(list, func(s string) bool { return s == id })
	}
	out := rewriteLua(string(data), list)
	next, err := Parse([]byte(out))
	if err != nil || !slices.Equal(next.Plugins.Lua, list) {
		return false, fmt.Errorf("cannot edit plugins.lua automatically; edit %s by hand", filepath.Base(path))
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".config-*")
	if err != nil {
		return false, err
	}
	defer func() { _ = os.Remove(tmp.Name()) }()
	if _, err = tmp.WriteString(out); err == nil {
		err = tmp.Chmod(0o600)
	}
	if cerr := tmp.Close(); err == nil {
		err = cerr
	}
	if err != nil {
		return false, err
	}
	return true, os.Rename(tmp.Name(), path)
}

func luaLine(list []string) string {
	q := make([]string, len(list))
	for i, s := range list {
		q[i] = fmt.Sprintf("%q", s)
	}
	return "lua = [" + strings.Join(q, ", ") + "]"
}

// rewriteLua replaces the `lua = [...]` statement of the [plugins] table, or
// adds one.
func rewriteLua(text string, list []string) string {
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
		if table != "plugins" || !luaKey.MatchString(lines[i]) {
			continue
		}
		end := i
		for depth := bracketDepth(lines[i]); depth > 0 && end+1 < len(lines); {
			end++
			depth += bracketDepth(lines[end])
		}
		return strings.Join(append(append(slices.Clone(lines[:i]), luaLine(list)), lines[end+1:]...), "\n")
	}
	if hdr >= 0 {
		return strings.Join(append(append(slices.Clone(lines[:hdr+1]), luaLine(list)), lines[hdr+1:]...), "\n")
	}
	return strings.TrimRight(text, "\n") + "\n\n[plugins]\n" + luaLine(list) + "\n"
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
