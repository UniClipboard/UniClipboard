// Command unic syncs the clipboard between your devices, peer to peer.
//
//	unicd               the background node (a separate program)
//	unic addr            print this device's address
//	unic code            show a 6-digit pairing code
//	unic join <code>     pair with the device showing that code
//	unic pair <addr>     pair with another device
//	unic config [path]   show the settings in effect
//	unic history | h     browse the local clipboard history
//	unic status | peers | unpair <id> | copy | paste | watch
package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"os/exec"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	api "github.com/uniclipboard/unic/internal/localapi"
)

const usage = `unic - clipboard sync between your devices

Usage: unic <command>

The background node is a separate program, unicd; start it first.

  submit [flags] [text]  submit text, stdin, -f files, or --png PATH without a clipboard
  submission ID    query a submission by request_id
  received         list, show or dismiss transient received content
  capabilities     show system clipboard capabilities
  status           show this device and its peers
  addr             print this device's address (treat it like a password)
  code             show a 6-digit code another device can join with
  join <code>      pair with the device showing that code
  pair <address>   pair with another device by its full address
  peers            list paired devices
  unpair <id>      forget a paired device
  requests         devices waiting for your approval
  approve <id> <fingerprint>  accept after checking the other device
  reject <id>      dismiss a waiting device
  history, h       list, show, delete and tag clipboard history (unic h help)
  copy [text]      put text (or stdin, or -f files) on the clipboard
  paste            print the clipboard's text
  watch            print sync events as they happen
  config           show settings; set <key> <TOML-value>; apply <file>; --json; path
  logs level       query or temporarily override local/remote log levels
  diagnostics telemetry  show telemetry state and dropped records
  cache [--json]   show default-cache bytes, reservations, and protected files
  plugin           list local plugins; enable or disable one (unic plugin help)
  migrate [flags]  import local UniClipboard v1 history (stop unicd first)

Settings live in config.toml in the state directory; saving it applies it at once.
Set UNIC_HOME to run a separate profile, UNIC_RENDEZVOUS_URL to use another
pairing code service.
`

func main() {
	showUpgradeNotice(os.Args[1:])
	if releaseinfo.Print(os.Stdout, "unic", "cli", os.Args[1:]) {
		return
	}
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
	var err error
	switch cmd, args := os.Args[1], os.Args[2:]; cmd {
	case "daemon":
		err = fmt.Errorf("the daemon is a separate program: run `unicd`")
	case "history", "h":
		err = runHistory(args)
	case "plugin":
		err = runPlugin(args)
	case "migrate":
		err = runMigration(args)
	case "help", "-h", "--help":
		fmt.Print(usage)
	default:
		err = runCLI(cmd, args)
	}
	if err != nil {
		asJSON := false
		for _, arg := range os.Args[1:] {
			if arg == "--json" {
				asJSON = true
			}
		}
		if asJSON {
			_ = json.NewEncoder(os.Stderr).Encode(map[string]string{"error": err.Error(), "reason": api.ReasonOf(err)})
		} else {
			fmt.Fprintln(os.Stderr, "unic:", err)
		}
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && exitErr.ExitCode() > 0 {
			os.Exit(exitErr.ExitCode())
		}
		os.Exit(1)
	}
}
