// Command uni syncs the clipboard between your devices, peer to peer.
//
//	uni daemon          run the background node (menu bar / tray icon)
//	uni gui             open the window
//	uni addr            print this device's address
//	uni pair <addr>     pair with another device
//	uni status | peers | unpair <id> | copy | paste | watch
package main

import (
	"fmt"
	"os"
)

const usage = `uni - clipboard sync between your devices

Usage: uni <command>

  daemon           run the background node (keeps the clipboard in sync)
  gui              open the window
  status           show this device and its peers
  addr             print this device's address (treat it like a password)
  pair <address>   pair with another device
  peers            list paired devices
  unpair <id>      forget a paired device
  copy [text]      put text (or stdin, or -f files) on the clipboard
  paste            print the clipboard's text
  watch            print sync events as they happen

Set UNI_HOME to run a separate profile.
`

func main() {
	if len(os.Args) < 2 {
		fmt.Fprint(os.Stderr, usage)
		os.Exit(2)
	}
	var err error
	switch cmd, args := os.Args[1], os.Args[2:]; cmd {
	case "daemon":
		err = runDaemon()
	case "gui":
		err = runGUI()
	case "help", "-h", "--help":
		fmt.Print(usage)
	default:
		err = runCLI(cmd, args)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "uni:", err)
		os.Exit(1)
	}
}
