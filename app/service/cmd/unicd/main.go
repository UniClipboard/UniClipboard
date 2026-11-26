// Command unicd is the unic daemon: it watches the clipboard and keeps it in
// sync with every paired device. unic (the CLI and GUI) talks to it over a
// local socket.
package main

import (
	"errors"
	"fmt"
	"os"
	"strings"

	"github.com/uniclipboard/unic/engine"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/releaseinfo"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func main() {
	if releaseinfo.Print(os.Stdout, "unicd", "cli", os.Args[1:]) {
		return
	}
	mode, profile, ok := parseArgs(os.Args[1:])
	if !ok {
		fmt.Fprintln(os.Stderr, "usage: unicd [--profile NAME] [--desktop-owned|--cli-temporary]")
		os.Exit(2)
	}
	if profile != "" {
		if err := paths.SetProfile(profile); err != nil {
			fmt.Fprintln(os.Stderr, "unicd:", err)
			os.Exit(2)
		}
	}
	if err := runDaemon(mode); err != nil {
		fmt.Fprintln(os.Stderr, "unicd:", err)
		var engineError *engine.Error
		if errors.As(err, &engineError) && engineError.Code == engine.CodeInUse {
			os.Exit(servicelifecycle.ExitDirectoryInUse)
		}
		os.Exit(1)
	}
}

// parseArgs parses the optional --profile flag and lifetime mode. Plain unicd is standalone.
func parseArgs(args []string) (servicelifecycle.Mode, string, bool) {
	mode := servicelifecycle.ModeStandalone
	modeSet := false
	profile := ""
	for i := 0; i < len(args); i++ {
		arg := args[i]
		switch {
		case arg == "--desktop-owned":
			if modeSet {
				return "", "", false
			}
			mode, modeSet = servicelifecycle.ModeDesktop, true
		case arg == "--cli-temporary":
			if modeSet {
				return "", "", false
			}
			mode, modeSet = servicelifecycle.ModeCLITemporary, true
		case arg == "--profile" || arg == "-p":
			if profile != "" || i+1 >= len(args) || args[i+1] == "" {
				return "", "", false
			}
			i++
			profile = args[i]
		case strings.HasPrefix(arg, "--profile="):
			v := strings.TrimPrefix(arg, "--profile=")
			if profile != "" || v == "" {
				return "", "", false
			}
			profile = v
		case strings.HasPrefix(arg, "-p="):
			v := strings.TrimPrefix(arg, "-p=")
			if profile != "" || v == "" {
				return "", "", false
			}
			profile = v
		default:
			return "", "", false
		}
	}
	return mode, profile, true
}
