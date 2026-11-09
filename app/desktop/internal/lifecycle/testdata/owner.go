// Command owner exercises the production launcher without loading a GUI runtime.
package main

import (
	"context"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func main() {
	login := flag.Bool("login", false, "exercise persisted login intent with a file-backed OS fixture")
	requested := flag.Bool("requested", false, "requested login state")
	retry := flag.Bool("retry", false, "retry login registration")
	fail := flag.Bool("fail", false, "simulate native registration rejection")
	flag.Parse()
	dir, err := paths.Dir()
	if err != nil {
		panic(err)
	}
	if *login {
		native := filepath.Join(dir, "native-login")
		registration := lifecycle.LoginRegistration{
			Path:    filepath.Join(dir, "desktop-login.json"),
			Enabled: func() bool { _, err := os.Stat(native); return err == nil },
			SetEnabled: func(on bool) error {
				if *fail {
					return fmt.Errorf("fixture rejected registration")
				}
				if on {
					return os.WriteFile(native, []byte("enabled"), 0600)
				}
				if err := os.Remove(native); err != nil && !os.IsNotExist(err) {
					return err
				}
				return nil
			},
		}
		enabled, err := registration.Apply(*requested, *retry)
		fmt.Printf("enabled=%t\n", enabled)
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
		return
	}
	socket, err := paths.Socket()
	if err != nil {
		panic(err)
	}
	executable, err := lifecycle.Sibling()
	if err != nil {
		panic(err)
	}
	ctx, cancel := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer cancel()
	process, err := lifecycle.Start(ctx, dir, socket, executable, true)
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println("ready")
	select {
	case <-ctx.Done():
	case <-process.Done():
	}
	if err := process.Stop(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
