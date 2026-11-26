package wire

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

func TestSafeName(t *testing.T) {
	for in, want := range map[string]string{
		"a.txt": "a.txt", "../../etc/passwd": "passwd", `..\..\x.dll`: "x.dll",
		"/abs/path.png": "path.png", "": "file", "..": "file", ".": "file", "a\x00b": "ab",
	} {
		if got := SafeName(in); got != want {
			t.Errorf("SafeName(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestValidateLimits(t *testing.T) {
	for _, m := range []Clip{
		{Parts: []PartMeta{{Kind: "fmt", Name: "x", Size: MaxPartBytes + 1}}},
		{Parts: []PartMeta{{Kind: "file", Name: "x", Size: MaxFileBytes + 1}}},
		{Parts: []PartMeta{{Kind: "fmt", Name: "x", Size: -1}}},
		{Parts: []PartMeta{{Kind: "weird", Name: "x"}}},
	} {
		if m.Validate() == nil {
			t.Errorf("expected %+v to be rejected", m)
		}
	}
}

func TestFits(t *testing.T) {
	m := Clip{Parts: []PartMeta{{Kind: "fmt", Name: "text/plain", Size: 10}, {Kind: "file", Name: "a", Size: 100}}}
	if err := m.Fits(Limits{File: 100, Total: 110}); err != nil {
		t.Errorf("exactly at the limits should fit: %v", err)
	}
	if m.Fits(Limits{File: 99, Total: 1000}) == nil {
		t.Error("file over limit should not fit")
	}
	if m.Fits(Limits{File: 100, Total: 109}) == nil {
		t.Error("total over limit should not fit")
	}
}

func TestSanitizeTraceParentAndSendClipStream(t *testing.T) {
	valid := "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	if got := SanitizeTraceParent(valid); got != valid {
		t.Fatalf("SanitizeTraceParent(%q) = %q", valid, got)
	}
	for _, bad := range []string{
		"",
		"01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
		"00-00000000000000000000000000000000-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
		"00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
		"not-a-traceparent",
	} {
		if got := SanitizeTraceParent(bad); got != "" {
			t.Errorf("SanitizeTraceParent(%q) = %q, want empty", bad, got)
		}
	}

	// Malformed traceparent on Clip must NOT fail Clip.Validate (sync stays unbroken).
	malformed := Clip{T: "clip", TraceParent: "bad-value", Parts: []PartMeta{{Kind: "fmt", Name: "text/plain", Size: 2}}}
	if err := malformed.Validate(); err != nil {
		t.Fatalf("Clip.Validate rejected malformed traceparent: %v", err)
	}

	// SendClipStream writes valid traceparent and omits invalid/empty ones.
	var buf bytes.Buffer
	conn := NewConn(&buf)
	if err := conn.SendClipStream("id1", "orig1", 1, "h1", []OutPart{{Kind: "fmt", Name: "text/plain", Size: 2, R: strings.NewReader("hi")}}, nil, valid, nil); err != nil {
		t.Fatal(err)
	}
	typ, raw, err := NewConn(&buf).ReadRaw()
	if err != nil || typ != "clip" {
		t.Fatalf("ReadRaw: typ=%q err=%v", typ, err)
	}
	var got Clip
	if err := json.Unmarshal(raw, &got); err != nil {
		t.Fatal(err)
	}
	if got.TraceParent != valid {
		t.Fatalf("got traceparent %q, want %q", got.TraceParent, valid)
	}

	buf.Reset()
	if err := NewConn(&buf).SendClipStream("id2", "orig1", 1, "h2", []OutPart{{Kind: "fmt", Name: "text/plain", Size: 2, R: strings.NewReader("hi")}}, nil, "malformed", nil); err != nil {
		t.Fatal(err)
	}
	_, raw, err = NewConn(&buf).ReadRaw()
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Contains(raw, []byte("traceparent")) {
		t.Fatalf("expected malformed/empty traceparent to be omitted, got %s", raw)
	}
}
