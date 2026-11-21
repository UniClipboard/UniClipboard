// Command boardui-demo opens an independent native BoardUI settings fixture.
package main

import (
	"flag"
	"fmt"
	"os"

	"github.com/egoist/mygo"
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/boardui/showcase"
)

func main() {
	theme := flag.String("theme", "light", "light or dark")
	opened := flag.Bool("open", true, "show the settings dialog immediately")
	width := flag.Int("width", 800, "client width in DIPs")
	height := flag.Int("height", 760, "client height in DIPs")
	flag.Parse()
	if (*theme != "light" && *theme != "dark") || *width < 320 || *height < 640 {
		fmt.Fprintln(os.Stderr, "invalid theme or window size")
		os.Exit(2)
	}
	a := showcase.New()
	a.Dark = *theme == "dark"
	a.Open = *opened
	mygo.App.SetName("BoardUI Native Demo")
	mygo.App.WhenReady(func() {
		mygo.NewWindow(mygo.WindowOptions{Title: "BoardUI Native Demo", Width: *width, Height: *height, MinWidth: 320, MinHeight: 640, Content: ui.View(a.View)})
	})
	if err := mygo.App.Run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
