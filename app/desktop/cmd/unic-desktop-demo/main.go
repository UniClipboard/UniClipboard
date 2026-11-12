// Command unic-desktop-demo launches isolated desktop previews.
package main

import (
	"fmt"
	"os"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopdemo"
)

func main() {
	if err := desktopdemo.Run(os.Args[1:]); err != nil {
		fmt.Fprintln(os.Stderr, "unic-desktop-demo:", err)
		os.Exit(1)
	}
}
