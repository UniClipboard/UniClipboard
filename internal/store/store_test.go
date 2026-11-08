package store

import (
	"path/filepath"
	"testing"
)

func TestBindingEndpointAndTicketMark(t *testing.T) {
	s, err := Open(filepath.Join(t.TempDir(), "db"))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = s.Close() }()
	if _, err := s.AddDevice("d", []byte("pk"), ""); err != nil {
		t.Fatal(err)
	}
	b, err := s.AddBinding("b", "d", "inst", []byte("ctx"))
	if err != nil {
		t.Fatal(err)
	}
	if err := s.SetBindingEndpoint(b.ID, []byte("addr"), true); err != nil {
		t.Fatal(err)
	}
	if err := s.SetBindingEndpoint(b.ID, []byte("addr2"), false); err != nil {
		t.Fatal(err)
	}
	got, err := s.InstanceBinding("d", "inst")
	if err != nil || string(got.Endpoint) != "addr2" || !got.Ticket || got.Revision != b.Revision {
		t.Fatalf("binding = %+v, %v", got, err)
	}
	// Redirecting the instance resets the mark with the authorization.
	if b2, _ := s.AddBinding("x", "d", "inst", []byte("other")); b2.Revision == b.Revision {
		t.Fatal("redirect must bump the revision")
	}
	if got, _ := s.InstanceBinding("d", "inst"); got.Ticket {
		t.Fatal("redirect must clear the ticket mark")
	}
}
