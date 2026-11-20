//go:build darwin

package main

import (
	"os"
	"path/filepath"

	"github.com/egoist/mygo"
)

func detectLoginLaunch() (bool, error) {
	program, err := os.Executable()
	if err != nil {
		return false, err
	}
	contents := filepath.Dir(filepath.Dir(program))
	if filepath.Base(filepath.Dir(program)) != "MacOS" || filepath.Base(contents) != "Contents" || filepath.Ext(filepath.Dir(contents)) != ".app" {
		return false, nil
	}
	// SMAppService login events belong to the app launched by LaunchServices,
	// not to an exec child. Read the receipt after AppKit has processed them.
	mygo.App.SetName(desktopAppName)
	mygo.App.SetActivationPolicy(mygo.ActivationPolicyAccessory)
	login := false
	mygo.App.WhenReady(func() {
		login = mygo.App.WasOpenedAtLogin()
		mygo.App.Quit()
	})
	if err := mygo.App.Run(); err != nil {
		return false, err
	}
	return login, nil
}
