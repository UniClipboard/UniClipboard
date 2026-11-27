// Command unic syncs the clipboard between your devices, peer to peer.
//
//	unic init           initialize a profile for first use
//	unic daemon start   run the background service (unicd)
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

	"github.com/uniclipboard/unic/internal/climigration"
	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/releaseinfo"
)

// Exit codes beyond 0, 1 (failed) and 2 (usage).
const (
	exitIncomplete        = 4 // committed, but the send, approval or window did not complete
	exitCleanup           = 5 // the command finished, but stopping its temporary service failed
	exitMigrationRequired = 6 // explicit first-run profile initialization is missing
)

// globalJSON records whether --json was passed as a global flag before the command.
var globalJSON bool

func main() { os.Exit(run(os.Args[1:])) }

// extractGlobalFlags strips global --profile/-p (anywhere before "--") and
// leading global --json flags before command classification.
func extractGlobalFlags(args []string) (cmdArgs []string, profile string, leadingJSON bool, err error) {
	seenCmd := false
	for i := 0; i < len(args); i++ {
		arg := args[i]
		if arg == "--" {
			cmdArgs = append(cmdArgs, args[i:]...)
			break
		}
		if arg == "-p" || arg == "--profile" {
			if i+1 >= len(args) || args[i+1] == "" {
				return nil, "", false, usageError("flag needs an argument: " + arg)
			}
			i++
			profile = args[i]
			continue
		}
		if strings.HasPrefix(arg, "--profile=") {
			profile = strings.TrimPrefix(arg, "--profile=")
			if profile == "" {
				return nil, "", false, usageError("flag needs an argument: --profile")
			}
			continue
		}
		if strings.HasPrefix(arg, "-p=") {
			profile = strings.TrimPrefix(arg, "-p=")
			if profile == "" {
				return nil, "", false, usageError("flag needs an argument: -p")
			}
			continue
		}
		if !seenCmd && (arg == "--json" || arg == "-json" || arg == "--json=true" || arg == "-json=true") {
			leadingJSON = true
			continue
		}
		seenCmd = true
		cmdArgs = append(cmdArgs, arg)
	}
	return cmdArgs, profile, leadingJSON, nil
}

func run(args []string) int {
	// Reset process-global state for in-process test calls.
	_ = paths.SetProfile("")
	globalJSON = false
	current = nil

	if releaseinfo.Print(os.Stdout, "unic", "cli", args) {
		return 0
	}
	if len(args) < 1 {
		fmt.Fprint(os.Stderr, usageText())
		return 2
	}

	cmdArgs, profile, leadingJSON, err := extractGlobalFlags(args)
	if err != nil {
		return report(args, err)
	}
	if profile != "" {
		if err := paths.SetProfile(profile); err != nil {
			return report(args, usageError(err.Error()))
		}
	}
	globalJSON = leadingJSON

	if releaseinfo.Print(os.Stdout, "unic", "cli", cmdArgs) {
		return 0
	}
	if len(cmdArgs) < 1 {
		fmt.Fprint(os.Stderr, usageText())
		return 2
	}

	cmd, rest := cmdArgs[0], cmdArgs[1:]
	l, left, err := classify(cmd, rest)
	if err != nil {
		return report(args, err)
	}

	// Per-command -h/--help always prints locally without init checks or daemon acquisition.
	if l.Name != "help" && wantsHelp(left) {
		if text, ok := helpForCommand(l.Name); ok {
			fmt.Print(text)
			return 0
		}
	}

	if l.Name == "init" {
		return runInit(left, os.Stdout, os.Stderr, globalJSON)
	}
	if l.Name == "migrate" {
		if globalJSON && !jsonRequested(rest) {
			rest = append(append([]string{}, rest...), "--json")
		}
		return runMigration(rest, os.Stdin, os.Stdout, os.Stderr)
	}

	if requiresInitialization(l.Name) {
		dir, err := paths.CanonicalDir()
		if err != nil {
			return report(args, err)
		}
		if err = climigration.Required(dir); err != nil {
			return report(args, err)
		}
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt)
	defer stop()

	var c *api.Client
	if l.Requirement == reqLocal {
		socket, err := paths.ResolveSocket()
		if err != nil {
			return report(args, err)
		}
		c = deniedClient(socket)
	} else {
		dir, err := paths.Dir()
		if err != nil {
			return report(args, err)
		}
		socket, err := paths.Socket()
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

func requiresInitialization(leafName string) bool {
	if leafName == "help" || strings.HasSuffix(leafName, " help") {
		return false
	}
	switch leafName {
	case "init", "migrate", "doctor", "completion",
		"profile", "profile list", "profile path",
		"daemon status", "daemon stop", "daemon logs",
		"config path":
		return false
	}
	return true
}

// dispatch runs a classified leaf. args is everything after the first word; left is what follows the leaf's own words.
func dispatch(ctx context.Context, c *api.Client, l leaf, args, left []string) error {
	switch {
	case l.Name == "help":
		if len(left) == 1 && left[0] == "--json" {
			return writeCatalog(os.Stdout)
		}
		if len(left) == 1 && left[0] == "exit-codes" {
			fmt.Print(helpExitCodesText)
			return nil
		}
		if len(left) >= 1 {
			topic := strings.Join(left, " ")
			if to, ok := firstWord[topic]; ok {
				topic = to
			}
			if text, ok := helpForCommand(topic); ok {
				fmt.Print(text)
				return nil
			}
			return usageError(fmt.Sprintf("unknown help topic %q (try `unic help`)", topic))
		}
		fmt.Print(usageText())
		return nil
	case l.Name == "dev help":
		fmt.Print(devUsage)
		return nil
	case l.Name == "device help":
		fmt.Print(deviceUsage)
		return nil
	case l.Name == "channel help":
		fmt.Print(channelUsage)
		return nil
	case l.Name == "daemon help":
		fmt.Print(daemonUsage)
		return nil
	case l.Name == "profile help":
		fmt.Print(profileUsage)
		return nil
	case l.Name == "cloud help":
		fmt.Print(cloudUsage)
		return nil
	case strings.HasPrefix(l.Name, "cloud "):
		return runCloudCmd(ctx, strings.TrimPrefix(l.Name, "cloud "), left, globalJSON)
	case strings.HasPrefix(l.Name, "daemon "):
		return runDaemonCmd(ctx, strings.TrimPrefix(l.Name, "daemon "), left, globalJSON)
	case l.Name == "profile":
		return runProfileCmd(ctx, "list", left, globalJSON)
	case strings.HasPrefix(l.Name, "profile "):
		return runProfileCmd(ctx, strings.TrimPrefix(l.Name, "profile "), left, globalJSON)
	case l.Name == "doctor":
		return runDoctor(ctx, left, globalJSON)
	case l.Name == "completion":
		return runCompletionStdout(left)
	case l.Name == "plugin":
		return runPlugin(ctx, args)
	case strings.HasPrefix(l.Name, "plugin settings "):
		return runPluginSettings(ctx, c, l.Name, left)
	case strings.HasPrefix(l.Name, "history "):
		return runHistory(ctx, c, args)
	}
	return runCLI(ctx, c, l, left)
}

func report(args []string, err error) int {
	var silent *silentExitError
	if errors.As(err, &silent) {
		return silent.code
	}
	var required *climigration.RequiredError
	if errors.As(err, &required) {
		asJSON := jsonRequested(args)
		if asJSON {
			_ = json.NewEncoder(os.Stderr).Encode(map[string]string{
				"error":              required.Error(),
				"reason":             "migration_required",
				"next_command":       "unic init",
				"initialize_command": "unic init --json",
				"migrate_command":    "unic migrate --help",
			})
		} else {
			fmt.Fprintln(os.Stderr, errorLine(required.Error()))
		}
		return exitMigrationRequired
	}
	asJSON := jsonRequested(args)
	var uerr *usageErr
	if errors.As(err, &uerr) {
		if asJSON {
			_ = json.NewEncoder(os.Stderr).Encode(map[string]string{"error": uerr.Error(), "reason": "invalid_argument"})
		} else {
			fmt.Fprintln(os.Stderr, errorLine(uerr.Error()))
		}
		return 2
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

func jsonRequested(args []string) bool {
	if globalJSON {
		return true
	}
	for _, arg := range args {
		if arg == "--json" || arg == "-json" || arg == "--json=true" || arg == "-json=true" {
			return true
		}
	}
	return false
}
