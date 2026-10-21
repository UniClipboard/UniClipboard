//go:build !darwin && !windows

package main

import "os"

// systemLanguage is the display language the system prefers, as the quick panel reads it
// (apps/quick-panel/src/platform/fallback.rs): the first non-empty locale variable.
func systemLanguage() string {
	for _, name := range []string{"LC_ALL", "LC_MESSAGES", "LANG"} {
		if value := os.Getenv(name); value != "" {
			return value
		}
	}
	return ""
}
