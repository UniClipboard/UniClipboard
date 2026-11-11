// Command unic syncs the clipboard between your devices, peer to peer.
//
//	unicd               the background node (a separate program)
//	unic <command>      see `unic help`; every command declares whether it needs unicd
package main

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"os"

	"os/signal"
	"strings"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

// Exit codes beyond 0, 1 (failed) and 2 (usage).
const (
	exitIncomplete = 4 // committed, but the send, approval or window did not complete
	exitCleanup    = 5 // the command finished, but stopping its temporary service failed
)

func main() { os.Exit(run(os.Args[1:])) }

func run(args []string) int {
	showUpgradeNotice(args)
	if releaseinfo.Print(os.Stdout, "unic", "cli", args) {
		return 0
	}
	if len(args) < 1 {
		fmt.Fprint(os.Stderr, usageText())
		return 2
	}
	cmd, rest := args[0], args[1:]
	l, left, err := classify(cmd, rest)
	if err != nil {
		return report(args, err)
	}
	if l.Requirement == reqOffline {
		return runMigration(rest, os.Stdin, os.Stdout, os.Stderr)
	}
	socket, err := paths.Socket()
	if err != nil {
		return report(args, err)
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	var c *api.Client
	if l.Requirement == reqLocal {
		c = deniedClient(socket)
	} else {
		dir, err := paths.Dir()
		if err != nil {
			return report(args, err)
		}
		current = newSession(dir, socket)
		c = current.client
	}
	err = dispatch(ctx, c, l, rest, left)
	code := 0
	if err != nil {
		code = report(args, err)
	}
	// Cleanup has its own context and runs after command output is complete.
	if cleanup := current.Release(); cleanup != nil {
		fmt.Fprintln(os.Stderr, errorLine("cleanup: "+cleanup.Error()))
		if code == 0 {
			code = exitCleanup
		}
	}
	return code
}

// dispatch runs a classified leaf. args is everything after the first word; left is what follows the leaf's own words.
func dispatch(ctx context.Context, c *api.Client, l leaf, args, left []string) error {
	switch {
	case l.Name == "help":
		if len(left) == 1 && left[0] == "--json" {
			return writeCatalog(os.Stdout)
		}
		fmt.Print(usageText())
		return nil
	case l.Name == "dev help":
		fmt.Println(groupUsage["dev"])
		return nil
	case l.Name == "plugin":
		return runPlugin(args)
	case strings.HasPrefix(l.Name, "history "):
		return runHistory(ctx, c, args)
	}
	return runCLI(ctx, c, l, left)
}

func report(args []string, err error) int {
	asJSON := false
	for _, arg := range args {
		if arg == "--json" {
			asJSON = true
		}
	}
	// Acquisition failures are not HTTP failures; print the cause alone.
	var acquire *acquireError
	if errors.As(err, &acquire) {
		err = acquire
	}
	var urlErr *url.Error
	if errors.As(err, &urlErr) && acquire == nil {
		err = urlErr.Err
	}
	if asJSON {
		_ = json.NewEncoder(os.Stderr).Encode(map[string]string{"error": err.Error(), "reason": api.ReasonOf(err)})
	} else {
		fmt.Fprintln(os.Stderr, errorLine(err.Error()))
	}
	var incomplete *incompleteError
	if errors.As(err, &incomplete) {
		return exitIncomplete
	}
	return 1
}
