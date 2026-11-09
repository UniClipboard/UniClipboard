// Command unicd is the unic daemon: it watches the clipboard and keeps it in
// sync with every paired device. unic (the CLI and GUI) talks to it over a
// local socket.
package main

import (
	"fmt"
	"os"
)

func main() {
	if err := runDaemon(); err != nil {
		fmt.Fprintln(os.Stderr, "unicd:", err)
		os.Exit(1)
	}
}
