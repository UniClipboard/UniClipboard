// Command daemonpatch sends one enveloped JSON write to the daemon, through the client the GUI uses (connection file and token of the
// profile in the environment). The tray icon E2E uses it to change daemon settings from outside the GUI, so that a change the icon shows
// can only have come from the daemon.
package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"time"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

func main() {
	if len(os.Args) != 4 || (os.Args[1] != http.MethodPut && os.Args[1] != http.MethodPatch) {
		fmt.Fprintln(os.Stderr, "usage: daemonpatch PUT|PATCH PATH JSON")
		os.Exit(2)
	}
	var body any
	if err := json.Unmarshal([]byte(os.Args[3]), &body); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(2)
	}
	client, err := daemonclient.FromEnv()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	var raw json.RawMessage
	if err := client.Enveloped(ctx, daemonclient.Request{Method: os.Args[1], Path: os.Args[2], JSON: body}, &raw); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(string(raw))
}
