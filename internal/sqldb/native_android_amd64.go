package sqldb

// Android on amd64 blocks the raw lstat syscall in modernc's Linux libc.
// The NDK SQLite build uses Android's libc and enables FTS5 in build-mobile.sh.
import _ "github.com/mattn/go-sqlite3"

const (
	Driver           = "sqlite3"
	WriteOptions     = "?_busy_timeout=5000&_journal_mode=WAL&_foreign_keys=on"
	SearchOptions    = "?mode=ro&_busy_timeout=1000&_query_only=on&_foreign_keys=on"
	MigrationOptions = "?mode=ro&_query_only=on&_busy_timeout=5000"
)
