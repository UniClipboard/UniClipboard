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
	language := flag.String("language", "en", "Desktop interface language")
	notes := flag.Bool("notes", false, "Report release notes from the actual mygo update check")
	owner := flag.Bool("owner", false, "Report production installation update ownership")
	install := flag.Bool("install", false, "Use the actual mygo installation path")
	flag.Parse()
	if *owner {
		executable, err := os.Executable()
		if err != nil {
			panic(err)
		}
		fmt.Println(appupdates.Owner(executable))
		return
	}
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	mygo.App.SetVersion("2.0.0-build.1")
	source := &appupdates.Source{
		Base: *base, Target: runtime.GOOS + "-" + runtime.GOARCH, Key: *key,
		CurrentVersion: mygo.App.Version(),
		Channel:        func() string { return *channel },
		Language:       func() string { return *language },
	}
	err := func() error {
		if *install || *notes {
			mygo.Updater.SetSource(source)
			candidate, err := mygo.Updater.Check(ctx)
			if err != nil {
				return err
			}
			if candidate == nil {
				return fmt.Errorf("expected a newer numbered build")
			}
			if *notes {
				return json.NewEncoder(os.Stdout).Encode(candidate.Notes)
			}
			return candidate.Install(ctx, nil)
		}
		body, err := source.Manifest(ctx, false)
		if err != nil {
			return err
		}
		defer func() { _ = body.Close() }()
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
		defer func() { _ = archive.Close() }()
		_, err = io.Copy(io.Discard, archive)
		return err
	}()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if !*notes {
		fmt.Println(`{"status":"passed"}`)
	}
}
