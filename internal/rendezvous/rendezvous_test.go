package rendezvous_test

import (
	"bytes"
	"context"
	"net/http"
	"reflect"
	"testing"
	"time"

	"github.com/uniclipboard/unic/internal/rendezvous"
	"github.com/uniclipboard/unic/internal/rendezvous/rvtest"
)

func TestNormalizeCode(t *testing.T) {
	for in, want := range map[string]string{
		"482913":       "482-913",
		"482 913":      "482-913",
		" 482-913 ":    "482-913",
		"４８２９１３":       "482-913",
		"482‐913":      "482-913",
		"48291":        "",
		"4829134":      "",
		"48a913":       "",
		"tc-something": "",
	} {
		got, ok := rendezvous.NormalizeCode(in)
		if got != want || ok != (want != "") {
			t.Errorf("NormalizeCode(%q) = %q, %v; want %q", in, got, ok, want)
		}
	}
}

func client(s *rvtest.Server) *rendezvous.Client {
	return &rendezvous.Client{URL: s.URL, HTTP: s.Client()}
}

func TestRoundTrip(t *testing.T) {
	s := rvtest.New()
	defer s.Close()
	c := client(s)
	ctx := context.Background()

	want := testTicket()
	code, err := c.Create(ctx, want)
	if err != nil {
		t.Fatal(err)
	}
	if left := time.Until(code.Expires); left <= 0 || left > rendezvous.MaxLifetime {
		t.Fatalf("expires in %v", left)
	}
	got, err := c.Resolve(ctx, code.Display())
	if err != nil || !reflect.DeepEqual(got, want) {
		t.Fatalf("Resolve = %+v, %v; want %+v", got, err, want)
	}
	c.Consume(ctx, code.Code)
	if _, err := c.Resolve(ctx, code.Code); rendezvous.ReasonOf(err) != rendezvous.Expired {
		t.Fatalf("after consume: %v", err)
	}
}

func TestReasons(t *testing.T) {
	s := rvtest.New()
	defer s.Close()
	c := client(s)
	ctx := context.Background()

	if _, err := c.Resolve(ctx, "12345"); rendezvous.ReasonOf(err) != rendezvous.Incomplete {
		t.Errorf("short code: %v", err)
	}
	if _, err := c.Resolve(ctx, "111111"); rendezvous.ReasonOf(err) != rendezvous.Expired {
		t.Errorf("unknown code: %v", err)
	}
	// A website transfer ticket is not a unic ticket.
	s.Put("222-222", `{"v":1,"kind":"uc-web-try","c":"tcEXAMPLEADDRESS0123456789","k":"AAAAAAAAAAAAAAAAAAAAAA"}`)
	if _, err := c.Resolve(ctx, "222222"); rendezvous.ReasonOf(err) != rendezvous.Invalid {
		t.Errorf("web ticket: %v", err)
	}
	for status, want := range map[int]rendezvous.Reason{
		http.StatusTooManyRequests:     rendezvous.RateLimited,
		http.StatusInternalServerError: rendezvous.Unavailable,
	} {
		s.SetFail(status)
		if _, err := c.Create(ctx, testTicket()); rendezvous.ReasonOf(err) != want {
			t.Errorf("status %d: %v", status, err)
		}
	}
	s.SetFail(0)
	s.Close()
	if _, err := c.Resolve(ctx, "333333"); rendezvous.ReasonOf(err) != rendezvous.Unavailable {
		t.Errorf("server down: %v", err)
	}
}

func TestLocalDeadlineIgnoresServerClock(t *testing.T) {
	s := rvtest.New()
	defer s.Close()
	s.TTL = 24 * time.Hour // a server clock far ahead must not stretch the code's life
	code, err := client(s).Create(context.Background(), testTicket())
	if err != nil {
		t.Fatal(err)
	}
	if left := time.Until(code.Expires); left > rendezvous.MaxLifetime {
		t.Fatalf("expires in %v, more than the fixed TTL", left)
	}
}

func testTicket() rendezvous.Ticket {
	return rendezvous.Ticket{Transport: rendezvous.TransportPeerSessions,
		Device: bytes.Repeat([]byte{7}, 32), Access: []byte("tcEXAMPLEADDRESS0123456789"), Token: rendezvous.NewToken()}
}

func TestTextFormRoundTripsAndRejectsOthers(t *testing.T) {
	want := testTicket()
	got, ok := rendezvous.ParseText("  " + want.Text()[:20] + "\n" + want.Text()[20:] + " ")
	if !ok || !reflect.DeepEqual(got, want) {
		t.Fatalf("ParseText = %+v, %v", got, ok)
	}
	old := `{"v":1,"kind":"unic-pair","c":"addr","k":"AAAAAAAAAAAAAAAAAAAAAA"}`
	for _, bad := range []string{"", "unic-pair:", "unic-pair:!!!", rendezvous.TextPrefix + "e30", old,
		"tcEXAMPLEADDRESS0123456789"} {
		if _, ok := rendezvous.ParseText(bad); ok {
			t.Errorf("ParseText(%q) accepted", bad)
		}
	}
	if _, ok := rendezvous.ParseTicket(old); ok {
		t.Error("a v1 ticket must not parse")
	}
	big := want
	big.Access = bytes.Repeat([]byte{1}, rendezvous.MaxAccess+1)
	if _, ok := rendezvous.ParseTicket(big.Encode()); ok {
		t.Error("an oversized access description parsed")
	}
}
