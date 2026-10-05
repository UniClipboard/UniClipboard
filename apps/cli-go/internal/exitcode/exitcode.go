// Package exitcode holds the CLI's stable process exit codes.
package exitcode

const (
	// Success is a successful run.
	Success = 0
	// Error is a general error.
	Error = 1
	// DaemonUnreachable means the daemon is not running or unreachable.
	DaemonUnreachable = 5
	// NoMatch means no clipboard entry matched the selector (`get`).
	NoMatch = 6
	// ContentUnavailable means a matched entry's payload is unavailable (`get`).
	ContentUnavailable = 7
)
