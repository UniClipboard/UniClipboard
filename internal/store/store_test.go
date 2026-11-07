package store

import (
	"database/sql"
	"path/filepath"
	"testing"
)

// A database written by a version without by_code opens and gains it.
func TestMigratesOldPendingTable(t *testing.T) {
	path := filepath.Join(t.TempDir(), "db")
	old, err := sql.Open("sqlite", "file:"+path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := old.Exec(`CREATE TABLE pending (id TEXT PRIMARY KEY, name TEXT NOT NULL, addr TEXT NOT NULL,
		via TEXT NOT NULL DEFAULT '', state TEXT NOT NULL DEFAULT 'pending', seen INTEGER NOT NULL);
		INSERT INTO pending (id, name, addr, seen) VALUES ('x', 'old', 'addr', 1)`); err != nil {
		t.Fatal(err)
	}
	old.Close()

	for i := 0; i < 2; i++ { // and again: migrations must be repeatable
		s, err := Open(path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := s.AddCandidate(Candidate{ID: "y", Name: "new", Addr: "a", ByCode: true}, true); err != nil {
			t.Fatal(err)
		}
		cs, err := s.Candidates()
		if err != nil || len(cs) != 2 || cs[0].ByCode || !cs[1].ByCode {
			t.Fatalf("candidates = %+v, %v", cs, err)
		}
		s.Close()
	}
}
