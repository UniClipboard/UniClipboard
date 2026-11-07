package wire

import "testing"

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
