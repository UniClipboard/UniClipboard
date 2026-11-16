package desktopui

import (
	"sync"

	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/appicons"
)

var appIcon = sync.OnceValue(func() ui.ImageSource {
	src, err := ui.DecodeImage(appicons.PNG)
	if err != nil {
		panic(err)
	}
	return src
})
