// Command updateprobe exercises the production adapter and native mygo updater without a GUI.
package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"
	"time"

	"github.com/egoist/mygo"
	"github.com/uniclipboard/unic/app/desktop/internal/appupdates"
)

func main() {
	base := flag.String("base", "", "Fixture server")
	key := flag.String("key", "", "Public key")
	channel := flag.String("channel", "stable", "Channel")
	install := flag.Bool("install", false, "Use the actual mygo installation path")
	flag.Parse()
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	source := &appupdates.Source{Base: *base, Target: runtime.GOOS + "-" + runtime.GOARCH, Key: *key, Channel: func() string { return *channel }}
	err := func() error {
		if *install {
			mygo.App.SetVersion("2.0.0-build.1")
			mygo.Updater.SetSource(source)
			candidate, err := mygo.Updater.Check(ctx)
			if err != nil {
				return err
			}
			if candidate == nil {
				return fmt.Errorf("expected a newer numbered build")
			}
			return candidate.Install(ctx, nil)
		}
		body, err := source.Manifest(ctx, false)
		if err != nil {
			return err
		}
		defer body.Close()
		var manifest struct {
			URL string `json:"url"`
		}
		if err = json.NewDecoder(body).Decode(&manifest); err != nil {
			return err
		}
		archive, err := source.Open(ctx, manifest.URL)
		if err != nil {
			return err
		}
		defer archive.Close()
		_, err = io.Copy(io.Discard, archive)
		return err
	}()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Println(`{"status":"passed"}`)
}
