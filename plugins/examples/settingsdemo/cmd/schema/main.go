// Command schema emits settingsdemo's static schema. It does not start a
// plugin process and is a developer tool, not an installer hook.
package main

import (
	"fmt"
	"os"

	"github.com/uniclipboard/unic/plugin/settings"
	"github.com/uniclipboard/unic/plugins/examples/settingsdemo"
)

func main() {
	data, err := settings.Generate[settingsdemo.Config]()
	if err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	if len(os.Args) == 1 {
		fmt.Println(string(data))
		return
	}
	for _, path := range os.Args[1:] {
		if err := os.WriteFile(path, append(data, '\n'), 0o644); err != nil {
			fmt.Fprintln(os.Stderr, "cannot write schema")
			os.Exit(1)
		}
	}
}
