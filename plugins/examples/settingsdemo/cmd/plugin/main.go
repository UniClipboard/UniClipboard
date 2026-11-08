package main

import (
	"os"

	"github.com/uniclipboard/unic/plugin"
	"github.com/uniclipboard/unic/plugins/examples/settingsdemo"
)

func main() {
	if plugin.Serve(&settingsdemo.Plugin{}) != nil {
		os.Exit(1)
	}
}
