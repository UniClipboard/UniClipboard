// Command daemonget prints the daemon's answer to an enveloped GET, read through the same client the GUI uses
// (connection file / token of the profile in the environment). The tray E2E uses it as the authoritative state
// of the daemon, independent of any menu snapshot.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: daemonget PATH")
		os.Exit(2)
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	var raw json.RawMessage
	if err := client.Get(ctx, os.Args[1], &raw); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(raw))
}
