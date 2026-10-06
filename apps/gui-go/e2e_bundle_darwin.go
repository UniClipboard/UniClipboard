//go:build e2e && darwin

package main

import "github.com/wailsapp/wails/v3/pkg/mac"

func bundleIdentifier() string { return mac.GetBundleID() }
