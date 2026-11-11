// Command unic-desktop runs the GUI and the system clipboard adapter.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"time"

	"github.com/uniclipboard/unic/internal/releaseinfo"

	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	config "github.com/uniclipboard/unic/internal/localconfig"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopdemo"
	api "github.com/uniclipboard/unic/internal/localapi"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func client() (*api.Client, error) {
	s, e := paths.Socket()
	if e != nil {
		return nil, e
	}
	return api.NewClient(s), nil
}
func main() {
	if releaseinfo.Print(os.Stdout, "unic-desktop", "desktop", os.Args[1:]) {
		return
	}
	var err error
	if len(os.Args) > 1 && os.Args[1] == "--demo" {
		err = desktopdemo.Run(os.Args[2:])
	} else if len(os.Args) > 1 {
		err = runDesktop(os.Args[1:])
	} else {
		err = runDesktop(nil)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "unic-desktop:", err)
		os.Exit(1)
	}
}

func runDesktop(args []string) error {
	flags := flag.NewFlagSet("unic-desktop", flag.ContinueOnError)
	mode := flags.String("mode", "", "normal or lightweight; overrides desktop.mode for this launch")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if flags.NArg() != 0 {
		return fmt.Errorf("usage: unic-desktop [--mode normal|lightweight] [--demo ...]")
	}
	path, err := paths.Config()
	if err != nil {
		return err
	}
	settings, err := config.Open(path, func(string, ...any) {})
	if err != nil {
		return err
	}
	selected := settings.Current().Desktop.Mode
	if *mode != "" {
		selected = *mode
	}
	if selected != "normal" && selected != "lightweight" {
		return fmt.Errorf("mode must be normal or lightweight")
	}
	if selected == "normal" {
		return runGUI(settings)
	}
	dir, err := paths.Dir()
	if err != nil {
		return err
	}
	socket, err := paths.Socket()
	if err != nil {
		return err
	}
	executable, err := lifecycle.Sibling()
	if err != nil {
		return err
	}
	ctx, cancel := context.WithTimeout(context.Background(), 45*time.Second)
	defer cancel()
	_, err = lifecycle.Start(ctx, dir, socket, executable, false)
	return err
}

func trunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
