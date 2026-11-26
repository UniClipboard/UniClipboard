package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"text/tabwriter"
	"time"

	"github.com/BurntSushi/toml"

	api "github.com/uniclipboard/unic/internal/localapi"

	"github.com/uniclipboard/unic/app/service/internal/luascripts"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

const pluginUsage = `usage: unic plugin <command>

  install <url> [--ref REF] [--path DIR] [--build] [--replace] [--enable]
  instance add <implementation> <id>  create a disabled instance with independent settings
  list                show installed and configured plugins
  enable <id>         enable at the next service restart
  disable <id>        disable at the next service restart
  uninstall <id>      remove a GitHub installation
  rollback <id>       select the previous installed commit
  settings show <id>  print the Schema and redacted settings
  settings save <id> <json-file|->  read {"values":{...}} from a file or stdin

A Lua plugin is a directory holding spec.toml and its script. Put it under
the plugins directory (see 'unic plugin list') and it shows up here, but it
does not run until it is enabled. Enabling takes effect when unicd restarts.
`

// runPlugin works on files only, so it does not need the daemon.
func runPlugin(ctx context.Context, args []string) error {
	root, err := paths.LuaDir()
	if err != nil {
		return err
	}
	cfg, err := paths.Config()
	if err != nil {
		return err
	}
	if len(args) > 0 {
		switch args[0] {
		case "install":
			return pluginInstall(ctx, args[1:], root, cfg, os.Stdout)
		case "rollback":
			if len(args) != 2 || !luascripts.ValidID(args[1]) {
				return errors.New("usage: unic plugin rollback <id>")
			}
			if err := luascripts.Rollback(root, args[1]); err != nil {
				return err
			}
			_, err = fmt.Fprintln(os.Stdout, args[1], "rolled back; run `unic daemon restart` to apply")
			return err
		case "uninstall":
			if len(args) != 2 || !luascripts.ValidID(args[1]) {
				return errors.New("usage: unic plugin uninstall <id>")
			}
			return luascripts.Uninstall(root, args[1], func() error {
				return pluginSet(false, args[1], root, cfg, io.Discard)
			})
		case "list", "ls":
			socket, err := paths.Socket()
			if err == nil {
				query, cancel := context.WithTimeout(ctx, 700*time.Millisecond)
				list, err := api.NewClient(socket).Plugins(query)
				cancel()
				if err == nil {
					return printRunningPlugins(os.Stdout, list)
				}
			}
		}
	}
	return pluginCmd(args, root, cfg, os.Stdout)
}

func pluginCmd(args []string, root, cfgPath string, out io.Writer) error {
	if len(args) == 0 {
		return errors.New(strings.TrimRight(pluginUsage, "\n"))
	}
	switch args[0] {
	case "instance":
		if len(args) != 4 || args[1] != "add" {
			return errors.New("usage: unic plugin instance add IMPLEMENTATION ID")
		}
		found, _ := luascripts.Scan(root)
		if _, err := luascripts.ResolveInstance(found, args[2], args[3]); err != nil {
			return err
		}
		m, err := config.Open(cfgPath, func(string, ...any) {})
		if err != nil {
			return err
		}
		next, err := m.Current().AddBackendInstance(args[2], args[3])
		if err != nil {
			return err
		}
		var raw bytes.Buffer
		if err := toml.NewEncoder(&raw).Encode(next); err != nil {
			return err
		}
		if _, err := m.Replace(m.State().Revision, raw.Bytes()); err != nil {
			return err
		}
		_, err = fmt.Fprintln(out, args[3], "created; configure it, then enable it and run `unic daemon restart`")
		return err
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
	for id, in := range cfg.Plugins.Instances {
		if in.Implementation != "" {
			if s, err := luascripts.ResolveInstance(found, in.Implementation, id); err == nil {
				found[id] = s
			}
		}
	}
	enabled := map[string]bool{}
	for _, id := range cfg.ExternalPlugins() {
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
	for _, id := range cfg.ExternalPlugins() { // listed but nothing to run
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
	if !luascripts.ValidID(id) {
		return errors.New("invalid external plugin ID")
	}
	found, bad := luascripts.Scan(root)
	cfg, err := loadConfig(cfgPath)
	if err != nil {
		return err
	}
	implementation := cfg.PluginImplementation(id)
	if s, err := luascripts.ResolveInstance(found, implementation, id); err == nil {
		found[id] = s
	}
	if on {
		if _, ok := found[id]; !ok {
			for _, err := range bad {
				return fmt.Errorf("%q is not usable: %w", id, err)
			}
			return fmt.Errorf("%q was not found in %s", id, root)
		}
		if found[id].Schema != nil {
			if _, err := found[id].RuntimeConfig(root, nil); err != nil {
				return fmt.Errorf("configure %s before enabling it: %w", id, err)
			}
		}

	}
	if _, err := os.Stat(cfgPath); errors.Is(err, os.ErrNotExist) {
		if err := config.WriteTemplate(cfgPath); err != nil {
			return err
		}
	}
	var changed bool
	switch {
	case !on:
		changed, err = config.DisableExternalPlugin(cfgPath, id)
	case found[id].Commit != "":
		changed, err = config.SetOrderedPluginEnabled(cfgPath, id, true)
	default:
		changed, err = config.SetLuaEnabled(cfgPath, id, true)
	}

	switch {
	case err != nil:
		return err
	case !changed && on:
		_, _ = fmt.Fprintln(out, id, "is already enabled")
	case !changed:
		_, _ = fmt.Fprintln(out, id, "is not enabled")
	default:
		_, _ = fmt.Fprintf(out, "%s %sd in %s; run `unic daemon restart` to apply\n", id, map[bool]string{true: "enable", false: "disable"}[on], cfgPath)
	}
	return nil
}

func pluginInstall(ctx context.Context, args []string, root, cfg string, out io.Writer) error {
	if len(args) == 0 {
		return errors.New(strings.TrimRight(pluginUsage, "\n"))
	}
	opt := luascripts.InstallOptions{Repository: args[0]}
	enable := false
	for i := 1; i < len(args); i++ {
		switch args[i] {
		case "--enable":
			enable = true
		case "--replace":
			opt.Replace = true
		case "--build":
			opt.Build = true
		case "--ref", "--path":
			flag := args[i]
			i++
			if i == len(args) || args[i] == "" || strings.HasPrefix(args[i], "--") {
				return errors.New("missing plugin installation option value")
			}
			if flag == "--ref" {
				opt.Ref = args[i]
			} else {
				opt.Path = args[i]
			}
		default:
			return errors.New("unknown plugin installation option")
		}
	}
	ctx, cancel := context.WithTimeout(ctx, 5*time.Minute)
	defer cancel()
	id, err := luascripts.Install(ctx, root, opt)
	if err != nil {
		return err
	}
	if enable {
		if err := pluginSet(true, id, root, cfg, out); err != nil {
			return fmt.Errorf("%s installed but could not be enabled: %w", id, err)
		}
	}
	_, err = fmt.Fprintln(out, id, "installed")
	return err
}

func printRunningPlugins(out io.Writer, plugins []api.PluginInfo) error {
	w := tabwriter.NewWriter(out, 0, 0, 2, ' ', 0)
	if _, err := fmt.Fprintln(w, "ID\tIMPLEMENTATION\tVERSION\tSOURCE\tCONFIGURED\tRUNNING\tRESTART"); err != nil {
		return err
	}
	for _, p := range plugins {
		running := p.RunningVersion
		if running == "" {
			running = "-"
		}
		if _, err := fmt.Fprintf(w, "%s\t%s\t%s\t%s\t%t\t%s\t%t\n", p.ID, p.ImplementationID, p.Version, p.Source, p.Configured, running, p.Restart); err != nil {
			return err
		}
	}
	return w.Flush()
}

func runPluginSettings(ctx context.Context, client *api.Client, command string, args []string) error {
	save := command == "plugin settings save"
	if (!save && len(args) != 1) || (save && len(args) != 2) || !luascripts.ValidID(args[0]) {
		return errors.New(strings.TrimRight(pluginUsage, "\n"))
	}
	var edit api.PluginSettingsEdit
	if save {
		var source io.Reader = os.Stdin
		if args[1] != "-" {
			file, err := os.Open(args[1])
			if err != nil {
				return errors.New("could not open plugin settings input")
			}
			defer func() { _ = file.Close() }()
			source = file
		}
		dec := json.NewDecoder(io.LimitReader(source, 128<<10))
		dec.DisallowUnknownFields()
		if dec.Decode(&edit) != nil || dec.Decode(new(any)) != io.EOF || len(edit.Values) == 0 {
			return errors.New("invalid plugin settings input")
		}
	}
	state, err := client.PluginSettings(ctx, args[0])
	if err != nil {
		return err
	}
	if save {
		if edit.Revision == "" {
			edit.Revision = state.Revision
		}
		state, err = client.SavePluginSettings(ctx, args[0], edit)
		if err != nil {
			return err
		}
	}
	return json.NewEncoder(os.Stdout).Encode(state)
}
