//go:build !android || !amd64

// Package sqldb selects the platform's SQLite driver and connection options.
package sqldb

import _ "modernc.org/sqlite"

const (
	Driver           = "sqlite"
	WriteOptions     = "?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)&_pragma=foreign_keys(1)"
	SearchOptions    = "?mode=ro&_pragma=busy_timeout(1000)&_pragma=query_only(1)&_pragma=foreign_keys(1)"
	MigrationOptions = "?mode=ro&_pragma=query_only(1)&_pragma=busy_timeout(5000)"
)
