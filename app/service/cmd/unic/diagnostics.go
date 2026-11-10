package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	"github.com/uniclipboard/unic/internal/observability"
)

func runLogs(ctx context.Context, c *api.Client, args []string) error {
	if len(args) > 0 {
		switch args[0] {
		case "level":
			return runLogLevels(ctx, c, args)
		case "export":
			return runLogExport(ctx, args[1:])
		case "help", "--help", "-h":
			fmt.Println("usage: unic logs level [debug|info|warn|error|reset] [--target local|remote] [--for 15m]\n" + logExportUsage)
			return nil
		}
	}
	return fmt.Errorf("usage: unic logs level ... | unic logs export [--output FILE]")
}

func runLogLevels(ctx context.Context, c *api.Client, args []string) error {
	if len(args) == 0 || args[0] != "level" {
		return fmt.Errorf("usage: unic logs level [debug|info|warn|error|reset] [--target local|remote] [--for 15m]")
	}
	args = args[1:]
	if len(args) == 0 {
		state, err := c.LogLevels(ctx)
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(state)
	}
	level := args[0]
	fs := flag.NewFlagSet("logs level", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	target := fs.String("target", "local", "")
	duration := fs.Duration("for", 15*time.Minute, "")
	if err := fs.Parse(args[1:]); err != nil || fs.NArg() != 0 {
		return fmt.Errorf("invalid log level arguments")
	}
	if *target != "local" && *target != "remote" {
		return fmt.Errorf("target must be local or remote")
	}
	var state observability.LogLevelState
	var err error
	if level == "reset" {
		explicit := false
		fs.Visit(func(f *flag.Flag) { explicit = explicit || f.Name == "for" })
		if explicit {
			return fmt.Errorf("reset does not accept --for")
		}
		state, err = c.ResetLogLevel(ctx, *target)
	} else {
		if _, ok := observability.Level(level); !ok || *duration < time.Second || *duration > time.Hour || *duration%time.Second != 0 {
			return fmt.Errorf("invalid level or duration; use 1..3600 whole seconds")
		}
		state, err = c.SetLogLevel(ctx, *target, level, int(*duration/time.Second))
	}
	if err != nil {
		return err
	}
	return json.NewEncoder(os.Stdout).Encode(state)
}
