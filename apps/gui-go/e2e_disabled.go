//go:build !e2e

package main

import "github.com/wailsapp/wails/v3/pkg/application"

func e2eServices(*HostService) []application.Service { return nil }
