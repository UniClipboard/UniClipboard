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
	if len(args) < 1 {
		fmt.Fprint(os.Stderr, usageText())
		return 2
	}
	cmd, rest := args[0], args[1:]
	l, err := classify(cmd, rest)
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
	err = dispatch(ctx, c, cmd, rest)
	code := 0
	if err != nil {
		code = report(args, err)
	}
	// Cleanup has its own context and runs after command output is complete.
	if cleanup := current.Release(); cleanup != nil {
		fmt.Fprintln(os.Stderr, "unic: cleanup:", cleanup)
		if code == 0 {
			code = exitCleanup
		}
	}
	return code
}

func dispatch(ctx context.Context, c *api.Client, cmd string, args []string) error {
	switch cmd {
	case "history", "h":
		return runHistory(ctx, c, args)
	case "plugin":
		return runPlugin(args)
	case "help", "-h", "--help":
		if len(args) == 1 && args[0] == "--json" {
			return writeCatalog(os.Stdout)
		}
		fmt.Print(usageText())
		return nil
	}
	return runCLI(ctx, c, cmd, args)
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
		fmt.Fprintln(os.Stderr, "unic:", err)
	}
	var incomplete *incompleteError
	if errors.As(err, &incomplete) {
		return exitIncomplete
	}
	return 1
}
