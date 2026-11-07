// Command unic syncs the clipboard between your devices, peer to peer.
//
//	unicd               the background node (a separate program)
//	unic gui             open the window
//	unic addr            print this device's address
//	unic code            show a 6-digit pairing code
//	unic join <code>     pair with the device showing that code
//	unic pair <addr>     pair with another device
//	unic config [path]   show the settings in effect
//	unic status | peers | unpair <id> | copy | paste | watch
package main

import (
	"fmt"
	"os"
)

const usage = `unic - clipboard sync between your devices

Usage: unic <command>

The background node is a separate program, unicd; start it first.

  gui              open the window
  status           show this device and its peers
  addr             print this device's address (treat it like a password)
  code             show a 6-digit code another device can join with
  join <code>      pair with the device showing that code
  pair <address>   pair with another device by its full address
  peers            list paired devices
  unpair <id>      forget a paired device
  requests         devices waiting for your approval
  approve <id>     accept a waiting device
  reject <id>      dismiss a waiting device
  copy [text]      put text (or stdin, or -f files) on the clipboard
  paste            print the clipboard's text
  watch            print sync events as they happen
  config [path]    show the settings in effect (or the config file's path)

Settings live in config.toml in the state directory; saving it applies it at once.
Set UNIC_HOME to run a separate profile, UNIC_RENDEZVOUS_URL to use another
pairing code service.
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
	var err error
	switch cmd, args := os.Args[1], os.Args[2:]; cmd {
	case "daemon":
		err = fmt.Errorf("the daemon is a separate program: run `unicd`")
	case "gui":
		err = runGUI()
	case "help", "-h", "--help":
		fmt.Print(usage)
	default:
		err = runCLI(cmd, args)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "unic:", err)
		os.Exit(1)
	}
}
