//go:build !unix

package e2e

func peerStallCommands() map[string]func(*desktopScenario, []string) error { return nil }
