//go:build unic_e2e

package main

import (
	"net/url"
	"os"
)

func init() {
	if feed := os.Getenv("UNIC_E2E_DESKTOP_UPDATE_FEED"); feed != "" {
		u, err := url.Parse(feed)
		if err != nil || u.Scheme != "http" || u.Hostname() != "127.0.0.1" || u.User != nil {
			panic("the desktop update fixture requires a loopback feed")
		}
		desktopUpdateFeed = feed
	}
}
