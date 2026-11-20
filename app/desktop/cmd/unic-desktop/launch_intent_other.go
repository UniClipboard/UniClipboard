//go:build !darwin

package main

import "github.com/egoist/mygo"

func detectLoginLaunch() (bool, error) {
	// mygo retains the OS login marker after removing it from os.Args.
	return mygo.App.WasOpenedAtLogin(), nil
}
