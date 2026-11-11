package webtransfer

import (
	"encoding/json"
	"net/url"
	"regexp"
	"strings"
)

// TicketKind marks a browser transfer ticket in rendezvous. unic's own pairing
// tickets use "unic-pair"; each side rejects the other's.
const TicketKind = "uc-web-try"

// SiteURL is the browser transfer page. Links put the ticket in the fragment,
// which a browser never sends to a server.
const SiteURL = "https://try.uniclipboard.app"

var (
	addrPattern  = regexp.MustCompile(`^tc[A-Za-z0-9_-]+$`)
	tokenPattern = regexp.MustCompile(`^[A-Za-z0-9_-]{22}$`)
)

// Ticket is what a receiver needs: the sender's tailcat address and the token that
// authorizes the pull. Both are secrets.
type Ticket struct {
	Addr, Token string
}

// NewToken returns a fresh 128-bit token.
func NewToken() string { return RandomID(16) }

// Valid reports whether both fields have the browser's shape.
func (t Ticket) Valid() bool {
	return len(t.Addr) >= 18 && len(t.Addr) <= 2050 && addrPattern.MatchString(t.Addr) && tokenPattern.MatchString(t.Token)
}

// Link is the copyable form: SiteURL/#c=<addr>&k=<token>.
func (t Ticket) Link() string {
	return SiteURL + "/#c=" + t.Addr + "&k=" + t.Token
}

// Encode is the rendezvous body for a 6-digit code.
func (t Ticket) Encode() string {
	b, _ := json.Marshal(map[string]any{"v": 1, "kind": TicketKind, "c": t.Addr, "k": t.Token})
	return string(b)
}

// ParseTicket reads a rendezvous ticket. It returns false for anything but a
// browser transfer ticket, including unic pairing tickets.
func ParseTicket(raw string) (Ticket, bool) {
	var v struct {
		V    int    `json:"v"`
		Kind string `json:"kind"`
		C    string `json:"c"`
		K    string `json:"k"`
	}
	if json.Unmarshal([]byte(raw), &v) != nil || v.V != 1 || v.Kind != TicketKind {
		return Ticket{}, false
	}
	t := Ticket{v.C, v.K}
	return t, t.Valid()
}

// ParseLink reads a link or a bare fragment ("#c=…&k=…").
func ParseLink(s string) (Ticket, bool) {
	s = strings.TrimSpace(s)
	frag := s
	if u, err := url.Parse(s); err == nil && u.Fragment != "" {
		frag = u.Fragment
	}
	q, err := url.ParseQuery(strings.TrimPrefix(frag, "#"))
	if err != nil {
		return Ticket{}, false
	}
	t := Ticket{q.Get("c"), q.Get("k")}
	return t, t.Valid()
}
