//go:build ignore

// This probe is built inside the pinned v1 GUI module so it can invoke the
// same install functions as the old updater.
package main

import (
	"log"
	"os"
	"runtime"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

func main() {
	if len(os.Args) != 3 {
		log.Fatal("usage: legacy-alpha-install <verified archive> <installed path>")
	}
	archive, err := os.Open(os.Args[1])
	if err != nil {
		log.Fatal(err)
	}
	defer archive.Close()
	switch runtime.GOOS {
	case "darwin":
		err = update.Install(archive, os.Args[2])
	case "linux":
		err = update.InstallAppImage(archive, os.Args[2])
	default:
		log.Fatal("unsupported operating system")
	}
	if err != nil {
		log.Fatal(err)
	}
}
