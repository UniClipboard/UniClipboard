// Command unicd is the unic daemon: it watches the clipboard and keeps it in
// sync with every paired device. unic (the CLI and GUI) talks to it over a
// local socket.
package main

import (
	"errors"
	"fmt"
	"os"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func main() {
	if releaseinfo.Print(os.Stdout, "unicd", "cli", os.Args[1:]) {
		return
	}
	mode, ok := parseMode(os.Args[1:])
	if !ok {
		fmt.Fprintln(os.Stderr, "usage: unicd [--desktop-owned|--cli-temporary]")
		os.Exit(2)
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

// parseMode maps the launch flag to a lifetime mode. Plain unicd is standalone.
func parseMode(args []string) (servicelifecycle.Mode, bool) {
	switch {
	case len(args) == 0:
		return servicelifecycle.ModeStandalone, true
	case len(args) == 1 && args[0] == "--desktop-owned":
		return servicelifecycle.ModeDesktop, true
	case len(args) == 1 && args[0] == "--cli-temporary":
		return servicelifecycle.ModeCLITemporary, true
	}
	return "", false
}
