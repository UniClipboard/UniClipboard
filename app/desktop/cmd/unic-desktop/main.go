// Command unic-desktop runs the GUI and the system clipboard adapter.
package main

import (
	"fmt"
	"os"

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
	var err error
	if len(os.Args) > 1 {
		err = fmt.Errorf("usage: unic-desktop")
	} else {
		err = runGUI()
	}
	if err != nil {
		fmt.Fprintln(os.Stderr, "unic-desktop:", err)
		os.Exit(1)
	}
}

func trunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}
