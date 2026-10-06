//go:build !e2e

package main

import "github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"

type devUpdate struct {
	endpoints func(update.Channel) []string
	publicKey string
}

// devUpdateOverrides is empty in normal builds: the feed and the trusted key
// cannot be redirected by the environment.
func devUpdateOverrides() (devUpdate, bool) { return devUpdate{}, false }
