// Command unic-desktop runs the GUI and the system clipboard adapter.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	"encoding/json"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	config "github.com/uniclipboard/unic/internal/localconfig"

	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func client() (*api.Client, error) {
	s, e := paths.Socket()
	if e != nil {
		return nil, e
	}
	return api.NewClient(s), nil
}
func main() {
	if releaseinfo.Print(os.Stdout, "unic-desktop", "desktop", os.Args[1:]) {
		return
	}
	err := runDesktop(os.Args[1:])
	if err != nil {
		fmt.Fprintln(os.Stderr, "unic-desktop:", err)
		os.Exit(1)
	}
}

func runDesktop(args []string) error {
	// Child roles inherit explicit resolved paths. Preserve whether the original
	// launcher used a custom profile; injected child paths are not user overrides.
	if os.Getenv("UNIC_DESKTOP_PROFILE_KIND") == "" {
		kind := "default"
		if os.Getenv("UNIC_HOME") != "" || os.Getenv("UNIC_SOCK") != "" {
			kind = "custom"
		}
		if err := os.Setenv("UNIC_DESKTOP_PROFILE_KIND", kind); err != nil {
			return err
		}
	}
	flags := flag.NewFlagSet("unic-desktop", flag.ContinueOnError)
	mode := flags.String("mode", "", "normal or lightweight; overrides desktop.mode for this launch")
	role := flags.String("role", "", "tray, gui or quick-panel")
	command := flags.String("command", "", "send a private desktop role command")
	payload := flags.String("payload", "null", "JSON command payload")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return fmt.Errorf("unexpected positional argument")
	}
	if *command != "" {
		dir, err := paths.Dir()
		if err != nil {
			return err
		}
		ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
		defer cancel()
		var result any
		if *command == "probe" {
			result, err = desktopprocess.Probe(ctx, dir, desktopprocess.Role(*role))
		} else {
			if !json.Valid([]byte(*payload)) {
				return fmt.Errorf("invalid JSON payload")
			}
			err = desktopprocess.Call(ctx, dir, desktopprocess.Role(*role), *command, json.RawMessage(*payload), &result)
		}
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(result)
	}

	path, err := paths.Config()
	if err != nil {
		return err
	}
	settings, err := config.Open(path, func(string, ...any) {})
	if err != nil {
		return err
	}
	selected := settings.Current().Desktop.Mode
	if *mode != "" {
		selected = *mode
	}
	if selected != "normal" && selected != "lightweight" {
		return fmt.Errorf("mode must be normal or lightweight")
	}
	desktopLaunchMode = selected
	if *role != "" {
		return runRole(desktopprocess.Role(*role), settings)
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	if _, err = launchDesktopRole(ctx, dir, socket, desktopprocess.Tray, "--mode", selected); err != nil {
		return err
	}
	return desktopprocess.Call(ctx, dir, desktopprocess.Tray, "startup", selected, nil)
}

func trunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
