package rpc

import (
	"strings"
	"testing"
)

func TestClipTraceParentValidation(t *testing.T) {
	base := Clip{
		WireID: "wire-1",
		Origin: "dev-1",
		AtMs:   1,
		Hash:   strings.Repeat("a", 64),
		Parts:  []ClipPart{{Index: 0, Kind: "representation", MIME: "text/plain", Size: 5}},
	}
	if err := base.Validate(); err != nil {
		t.Fatalf("empty traceparent rejected: %v", err)
	}
	valid := base
	valid.TraceParent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
	if err := valid.Validate(); err != nil {
		t.Fatalf("valid traceparent rejected: %v", err)
	}
	for _, bad := range []string{
		"00-00000000000000000000000000000000-00f067aa0ba902b7-01",
		"00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
		"00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01",
		"01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
		"bad",
	} {
		c := base
		c.TraceParent = bad
		if err := c.Validate(); err == nil {
			t.Errorf("expected invalid traceparent %q to fail validation", bad)
		}
	}
}
