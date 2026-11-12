package desktopui

import (
	_ "embed"
	"sync"

	"github.com/egoist/mygo/ui"
)

//go:embed app-icon.png
var appIconPNG []byte

var appIcon = sync.OnceValue(func() ui.ImageSource {
	src, err := ui.DecodeImage(appIconPNG)
	if err != nil {
		panic(err)
	}
	return src
})
