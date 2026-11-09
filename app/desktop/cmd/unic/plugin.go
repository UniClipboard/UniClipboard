package main

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"text/tabwriter"

	"github.com/uniclipboard/unic/app/desktop/internal/config"
	"github.com/uniclipboard/unic/app/desktop/internal/luascripts"
	"github.com/uniclipboard/unic/app/desktop/internal/paths"
)

const pluginUsage = `usage: unic plugin <command>

  list                 plugins found in the plugins directory and whether they run
  enable <id>          add a plugin to plugins.lua in config.toml
  disable <id>         remove it again

A Lua plugin is a directory holding spec.toml and its script. Put it under
the plugins directory (see 'unic plugin list') and it shows up here, but it
does not run until it is enabled. Enabling takes effect when unicd restarts.
`

// runPlugin works on files only, so it does not need the daemon.
func runPlugin(args []string) error {
	root, err := paths.LuaDir()
	if err != nil {
		return err
	}
	cfg, err := paths.Config()
	if err != nil {
		return err
	}
	return pluginCmd(args, root, cfg, os.Stdout)
}

func pluginCmd(args []string, root, cfgPath string, out io.Writer) error {
	if len(args) == 0 {
		return errors.New(strings.TrimRight(pluginUsage, "\n"))
	}
	switch args[0] {
	case "list", "ls":
		return pluginList(root, cfgPath, out)
	case "enable", "disable":
		if len(args) != 2 {
			return fmt.Errorf("usage: unic plugin %s <id>", args[0])
		}
		return pluginSet(args[0] == "enable", args[1], root, cfgPath, out)
	case "help", "-h", "--help":
		_, _ = fmt.Fprint(out, pluginUsage)
		return nil
	}
	return fmt.Errorf("unknown plugin command %q\n%s", args[0], strings.TrimRight(pluginUsage, "\n"))
}

func loadConfig(path string) (config.Config, error) {
	b, err := os.ReadFile(path)
	if errors.Is(err, os.ErrNotExist) {
		return config.Default(), nil
	}
	if err != nil {
		return config.Config{}, err
	}
	return config.Parse(b)
}

func pluginList(root, cfgPath string, out io.Writer) error {
	cfg, err := loadConfig(cfgPath)
	if err != nil {
		return fmt.Errorf("config.toml: %w", err)
	}
	found, bad := luascripts.Scan(root)
	enabled := map[string]bool{}
	for _, id := range cfg.Plugins.Lua {
		enabled[id] = true
	}
	_, _ = fmt.Fprintln(out, "directory:", root)
	w := tabwriter.NewWriter(out, 0, 0, 2, ' ', 0)
	_, _ = fmt.Fprintln(w, "ID\tVERSION\tSTATE\tCAPABILITIES\tDIR")
	for _, id := range slices.Sorted(mapKeys(found)) {
		s := found[id]
		state := "disabled"
		if enabled[id] {
			state = "enabled"
		}
		_, _ = fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%s\n", id, s.Manifest.Version, state, strings.Join(s.Manifest.Capabilities, ","), filepath.Base(s.Dir))
	}
	for _, id := range cfg.Plugins.Lua { // listed but nothing to run
		if _, ok := found[id]; !ok {
			_, _ = fmt.Fprintf(w, "%s\t-\tenabled, MISSING\t-\t-\n", id)
		}
	}
	if err := w.Flush(); err != nil {
		return err
	}
	for _, dir := range slices.Sorted(mapKeys(bad)) {
		_, _ = fmt.Fprintf(out, "not usable: %v\n", bad[dir])
	}
	if len(found) == 0 && len(bad) == 0 {
		_, _ = fmt.Fprintln(out, "no plugins found")
	}
	return nil
}

func mapKeys[V any](m map[string]V) func(func(string) bool) {
	return func(yield func(string) bool) {
		for k := range m {
			if !yield(k) {
				return
			}
		}
	}
}

func pluginSet(on bool, id, root, cfgPath string, out io.Writer) error {
	if on {
		found, bad := luascripts.Scan(root)
		if _, ok := found[id]; !ok {
			for _, err := range bad {
				return fmt.Errorf("%q is not usable: %w", id, err)
			}
			return fmt.Errorf("%q was not found in %s", id, root)
		}
	}
	if _, err := os.Stat(cfgPath); errors.Is(err, os.ErrNotExist) {
		if err := config.WriteTemplate(cfgPath); err != nil {
			return err
		}
	}
	changed, err := config.SetLuaEnabled(cfgPath, id, on)
	switch {
	case err != nil:
		return err
	case !changed && on:
		_, _ = fmt.Fprintln(out, id, "is already enabled")
	case !changed:
		_, _ = fmt.Fprintln(out, id, "is not enabled")
	default:
		_, _ = fmt.Fprintf(out, "%s %sd in %s; restart unicd to apply\n", id, map[bool]string{true: "enable", false: "disable"}[on], cfgPath)
	}
	return nil
}
