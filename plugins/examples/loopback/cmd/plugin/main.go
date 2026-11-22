package main

import (
	"os"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugins/examples/loopback"
)

func main() {
	if plugin.Serve(loopback.New()) != nil {
		os.Exit(1)
	}
}
