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

func TestPendingRequestKeepsItsOriginWhenAskedAgain(t *testing.T) {
	s, err := Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	defer s.Close()
	if created, err := s.AddCandidate(Candidate{ID: "x", Name: "x", Addr: "a", ByCode: true}, true); err != nil || !created {
		t.Fatalf("first = %v, %v", created, err)
	}
	if _, err := s.db.Exec(`UPDATE pending SET seen = 1000 WHERE id = 'x'`); err != nil {
		t.Fatal(err)
	}
	// The device reconnects in the background, this time without the code.
	if created, err := s.AddCandidate(Candidate{ID: "x", Name: "x2", Addr: "b"}, true); err != nil || created {
		t.Fatalf("again = %v, %v", created, err)
	}
	c, ok, _ := s.Candidate("x")
	if !ok || !c.ByCode || c.Seen.Unix() != 1000 || c.Name != "x2" || c.Addr != "b" {
		t.Fatalf("candidate = %+v", c)
	}
	// After a rejection the same device asking again is a new request.
	s.RejectCandidate("x")
	if created, _ := s.AddCandidate(Candidate{ID: "x", Name: "x", Addr: "b"}, true); !created {
		t.Fatal("a rejected device asking again must be new")
	}
	if c, _, _ := s.Candidate("x"); c.ByCode || c.Seen.Unix() == 1000 {
		t.Fatalf("revived candidate kept old data: %+v", c)
	}
}
