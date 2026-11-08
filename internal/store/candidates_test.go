package store

import "testing"

func TestCandidates(t *testing.T) {
	s, _ := openTemp(t)
	pk := func(b byte) []byte { k := make([]byte, 32); k[0] = b; return k }
	for i, id := range []string{"intro-b", "intro-c"} {
		if _, err := s.AddDevice(id, pk(byte(i+1)), id); err != nil {
			t.Fatal(err)
		}
	}
	const day = int64(24 * 3600 * 1000)
	now := int64(1_000_000_000_000)
	c := func(dev string) Candidate {
		return Candidate{DeviceID: dev, Name: "n-" + dev, Address: []byte("addr-" + dev)}
	}
	if err := s.ReplaceVouches("intro-b", "i1", []Candidate{c("a"), c("x")}, now, 7*day); err != nil {
		t.Fatal(err)
	}
	if err := s.ReplaceVouches("intro-c", "i1", []Candidate{c("a")}, now+1, 7*day); err != nil {
		t.Fatal(err)
	}
	got, _ := s.Candidates(now+2, 7*day)
	if len(got) != 3 {
		t.Fatalf("want 3 vouches, got %+v", got)
	}
	// b stops vouching for x: it keeps its old time and ages out by itself.
	if err := s.ReplaceVouches("intro-b", "i1", []Candidate{c("a")}, now+day, 7*day); err != nil {
		t.Fatal(err)
	}
	if got, _ = s.Candidates(now+day+2, 7*day); len(got) != 3 {
		t.Fatalf("x lingers: %+v", got)
	}
	if got, _ = s.Candidates(now+8*day, 7*day); len(got) != 1 || got[0].DeviceID != "a" {
		t.Fatalf("x ages out: %+v", got)
	}
	// Dropping an introducer removes only its vouches.
	if err := s.DropIntroducer("intro-b"); err != nil {
		t.Fatal(err)
	}
	if got, _ = s.Candidates(now+day+2, 7*day); len(got) != 1 || got[0].IntroducerID != "intro-c" {
		t.Fatalf("after drop: %+v", got)
	}
	// A device with a record is no longer a candidate, and new vouches skip it.
	if _, err := s.AddDevice("a", pk(9), "a"); err != nil {
		t.Fatal(err)
	}
	if got, _ = s.Candidates(now+day+2, 7*day); len(got) != 0 {
		t.Fatalf("known device listed: %+v", got)
	}
	if err := s.ReplaceVouches("intro-c", "i1", []Candidate{c("a")}, now+day, 7*day); err != nil {
		t.Fatal(err)
	}
	if got, _ = s.Candidates(now+day+2, 7*day); len(got) != 0 {
		t.Fatalf("known device vouched: %+v", got)
	}

	// Ignore and requests.
	if err := s.SetCandidateIgnored("q", true, now); err != nil {
		t.Fatal(err)
	}
	if m, _ := s.IgnoredCandidates(); !m["q"] {
		t.Fatal("ignore lost")
	}
	_ = s.SetCandidateIgnored("q", false, now)
	if m, _ := s.IgnoredCandidates(); m["q"] {
		t.Fatal("restore failed")
	}
	_ = s.RequestCandidate("q", "i1", now)
	if rs, _ := s.CandidateRequests(now+day/2, day); len(rs) != 1 {
		t.Fatalf("request: %+v", rs)
	}
	if rs, _ := s.CandidateRequests(now+2*day, day); len(rs) != 0 {
		t.Fatalf("request outlived its window: %+v", rs)
	}

	// Caps.
	var many []Candidate
	for i := 0; i < 200; i++ {
		many = append(many, c(string(rune('A'+i/26))+string(rune('a'+i%26))))
	}
	for i := 0; i < 4; i++ { // vouch messages hold 64 at most
		if err := s.ReplaceVouches("intro-b", "i1", many[i*50:(i+1)*50], now+int64(i)+3*day, 7*day); err != nil {
			t.Fatal(err)
		}
	}
	if got, _ = s.Candidates(now+3*day+10, 7*day); len(got) > MaxCandidatesPerIntroducer {
		t.Fatalf("per-introducer cap: %d", len(got))
	}
}
