// Command unicd is the unic daemon: it watches the clipboard and keeps it in
// sync with every paired device. unic (the CLI and GUI) talks to it over a
// local socket.
package main

import (
	"errors"
	"fmt"
	"os"

	"github.com/uniclipboard/unic/engine"
	"github.com/uniclipboard/unic/internal/servicelifecycle"
)

func main() {
	if len(os.Args) > 2 || (len(os.Args) == 2 && os.Args[1] != "--desktop-owned") {
		fmt.Fprintln(os.Stderr, "usage: unicd [--desktop-owned]")
		os.Exit(2)
	}
	if err := runDaemon(); err != nil {
		fmt.Fprintln(os.Stderr, "unicd:", err)
		var engineError *engine.Error
		if errors.As(err, &engineError) && engineError.Code == engine.CodeInUse {
			os.Exit(servicelifecycle.ExitDirectoryInUse)
		}
		os.Exit(1)
	}
}
