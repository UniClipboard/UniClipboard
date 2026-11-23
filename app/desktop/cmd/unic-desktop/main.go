// Command unic-desktop runs the GUI and the system clipboard adapter.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"time"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	"encoding/json"
	"errors"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	"github.com/uniclipboard/unic/app/desktop/internal/setupflow"
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
	args := os.Args[1:]
	if filepath.Base(os.Args[0]) == "gui-go" {
		// The old macOS updater may pass restart flags to its original executable path.
		args = slices.DeleteFunc(slices.Clone(args), func(arg string) bool {
			return arg == "--resident-restart" || arg == "--background-restart"
		})
	}
	if releaseinfo.Print(os.Stdout, "unic-desktop", "desktop", args) {
		return
	}
	err := runDesktop(args)
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
	mode := flags.String("mode", "", "normal or lightweight background startup policy; defaults to desktop.mode")
	background := flags.Bool("background", false, "start without a GUI after setup; lightweight runs only unicd")
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
	desktopLaunchBackground = *background
	if *role != "" {
		selectedRole := desktopprocess.Role(*role)
		setRoleProcessName(selectedRole)
		return runRole(selectedRole, settings)
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
	backgroundLaunch := *background
	if !backgroundLaunch {
		login, err := detectLoginLaunch()
		if err != nil {
			return err
		}
		backgroundLaunch = login && (selected == "lightweight" || settings.Current().Desktop.StartInBackground)
	}
	flow, err := setupflow.Open(dir)
	if err != nil {
		return err
	}
	if backgroundLaunch && selected == "lightweight" && !flow.Required() {
		// A duplicate login/background request must not close an active UI.
		if _, err := desktopprocess.Probe(ctx, dir, desktopprocess.Tray); err == nil {
			return nil
		} else if !errors.Is(err, desktopprocess.ErrUnavailable) {
			return err
		}
		executable, err := lifecycle.Sibling()
		if err != nil {
			return err
		}
		owned, err := lifecycle.AcquireService(ctx, dir, socket, executable)
		if err != nil {
			return err
		}
		if useDaemonClipboard {
			client := api.NewClient(socket)
			if err := client.EnableDesktopClipboard(ctx); err != nil {
				if owned != nil {
					_ = owned.Stop()
				}
				return err
			}
			_, _ = client.EnableDesktopNotifications(ctx)
		}
		if owned != nil {
			return owned.Detach(ctx)
		}
		return nil
	}
	startupCommand := "startup"
	if backgroundLaunch {
		startupCommand = "background"
	}
	trayArgs := []string{"--mode", selected}
	if backgroundLaunch {
		trayArgs = append(trayArgs, "--background")
	}
	for {
		if _, err = launchDesktopRole(ctx, dir, socket, desktopprocess.Tray, trayArgs...); err != nil {
			return err
		}
		err = desktopprocess.Call(ctx, dir, desktopprocess.Tray, startupCommand, nil, nil)
		if !errors.Is(err, desktopprocess.ErrUnavailable) {
			return err
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(50 * time.Millisecond):
		}
	}
}

func trunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
