// Command role exercises the production role IPC with independent OS processes.
package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"os"
	"strings"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
)

func main() {
	role := flag.String("role", "tray", "role")
	command := flag.String("command", "", "command")
	flag.Parse()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	dir := os.Getenv("UNIC_HOME")
	if *command != "" {
		var result any
		var err error
		if *command == "status" {
			result, err = desktopprocess.Probe(ctx, dir, desktopprocess.Role(*role))
		} else {
			requested := strings.TrimPrefix(*command, "classify-")
			err = desktopprocess.Call(ctx, dir, desktopprocess.Role(*role), requested, nil, &result)
			if strings.HasPrefix(*command, "classify-") {
				result = map[string]any{"retryable_unavailable": errors.Is(err, desktopprocess.ErrUnavailable), "received_error": err != nil}
				err = nil
			}
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		_ = json.NewEncoder(os.Stdout).Encode(result)
		return
	}
	done := make(chan struct{}, 1)
	owner, err := desktopprocess.Acquire(ctx, dir, desktopprocess.Role(*role), func(_ context.Context, command string, _ json.RawMessage) (any, error) {
		if command == "retiring" {
			return nil, fmt.Errorf("role retirement underway: %w", desktopprocess.ErrUnavailable)
		}
		if command == "lookalike" {
			return nil, errors.New(desktopprocess.ErrUnavailable.Error())
		}
		if command == "close" {
			return desktopprocess.ReplyAfter(map[string]any{"pid": os.Getpid(), "command": command}, func() {
				cancel()
				select {
				case done <- struct{}{}:
				default:
				}
			}), nil
		}
		return map[string]any{"pid": os.Getpid(), "command": command}, nil
	})
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	defer func() { _ = owner.Close() }()
	<-done
}
