package main

import (
	"log"
	"os"

	"github.com/wailsapp/wails/v3/pkg/application"
	"github.com/wailsapp/wails/v3/pkg/events"
)

// failStartup tells the user why the application cannot start, then exits with status 1. A double-clicked AppImage
// has no terminal, so a stderr line alone would be invisible: a minimal Wails application shows the error dialog once
// its event loop is up (dialogs need it), and quits when the user dismisses it. The message also goes to stderr.
func failStartup(err error) {
	log.Print(err)
	app := application.New(application.Options{Name: "UniClipboard"})
	app.Event.OnApplicationEvent(events.Common.ApplicationStarted, func(*application.ApplicationEvent) {
		dialog := app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error())
		dialog.AddButton("OK").OnClick(app.Quit)
		dialog.Show()
	})
	if runErr := app.Run(); runErr != nil {
		log.Print(runErr)
	}
	os.Exit(1)
}
