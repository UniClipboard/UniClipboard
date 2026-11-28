package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"time"

	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
	"github.com/uniclipboard/unic/internal/logarchive"
)

const logExportUsage = "usage: unic dev logs export [--output FILE]"

func runLogExport(ctx context.Context, args []string) error {
	flags := flag.NewFlagSet("logs export", flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	output := flags.String("output", "unic-diagnostics-"+time.Now().UTC().Format("20060102T150405.000000000Z")+".zip", "")
	if err := flags.Parse(args); errors.Is(err, flag.ErrHelp) {
		fmt.Println(logExportUsage)
		return nil
	} else if err != nil {
		return errors.New(logExportUsage)
	}
	if flags.NArg() != 0 || *output == "" {
		return errors.New(logExportUsage)
	}
	profile, err := paths.CanonicalDir()
	if err != nil {
		return errors.New("cannot open the local profile")
	}
	var client *api.Client
	if socket, err := paths.ResolveSocket(); err == nil && socket != "" {
		client = api.NewClient(socket)
	}
	archive, err := logarchive.Export(ctx, profile, *output, "unic", client)
	if err != nil {
		return err
	}
	fmt.Println(archive)
	return nil
}
