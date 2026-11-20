// Command owner exercises the production launcher without loading a GUI runtime.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"path/filepath"
	"syscall"

	api "github.com/uniclipboard/unic/internal/localapi"

	"github.com/uniclipboard/unic/app/desktop/internal/lifecycle"
	config "github.com/uniclipboard/unic/internal/localconfig"
	paths "github.com/uniclipboard/unic/internal/localpaths"
)

func main() {
	login := flag.Bool("login", false, "exercise persisted login intent with a file-backed OS fixture")
	requested := flag.Bool("requested", false, "requested login state")
	retry := flag.Bool("retry", false, "retry login registration")
	fail := flag.Bool("fail", false, "simulate native registration rejection")
	acquire := flag.Bool("acquire", false, "acquire service without replacing standalone ownership")
	lightweight := flag.Bool("lightweight", false, "exercise legacy standalone service primitive without a graphical host")
	detach := flag.Bool("detach", false, "release this owned generation and verify Stop cannot terminate it")
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
	if *lightweight {
		path, err := paths.Config()
		if err == nil {
			_, err = config.Open(path, func(string, ...any) {})
		}
		if err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
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
	var process *lifecycle.Process
	if *acquire {
		process, err = lifecycle.AcquireService(ctx, dir, socket, executable)
	} else {
		process, err = lifecycle.Start(ctx, dir, socket, executable, !*lightweight)
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if *detach {
		if process == nil {
			panic("detach fixture requires owned generation")
		}
		before, err := api.NewClient(socket).Service(ctx)
		if err != nil {
			panic(err)
		}
		if err := process.Detach(ctx); err != nil {
			panic(err)
		}
		if err := process.Detach(ctx); err != nil {
			panic(err)
		}
		if err := process.Stop(); err != nil {
			panic(err)
		}
		after, err := api.NewClient(socket).Service(ctx)
		if err != nil {
			panic(err)
		}
		_ = json.NewEncoder(os.Stdout).Encode(map[string]any{"before": before, "after": after})
		return
	}
	fmt.Println("ready")
	if *lightweight {
		return
	}
	if *acquire {
		fmt.Printf("owned=%t\n", process != nil)
	}
	if process == nil {
		<-ctx.Done()
		return
	}
	select {
	case <-ctx.Done():
	case <-process.Done():
	}
	if err := process.Stop(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
