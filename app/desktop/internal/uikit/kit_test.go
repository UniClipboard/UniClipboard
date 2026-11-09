package uikit

import "testing"

func TestSemanticTokensResolve(t *testing.T) {
	for _, skin := range []SkinID{Linux, MacOS, Windows} {
		for _, dark := range []bool{false, true} {
			k := Kit{Skin: skin, Dark: dark}
			for name, tok := range semanticTokens[skin] {
				if !k.Has(tok) {
					t.Errorf("%s dark=%v: %s -> missing token %q", skin, dark, name, tok)
					continue
				}
				if _, ok := parseColor(k.Raw(tok)); !ok {
					t.Errorf("%s dark=%v: %s -> %q = %q is not a colour", skin, dark, name, tok, k.Raw(tok))
				}
			}
		}
	}
}

func TestParse(t *testing.T) {
	if c, ok := parseColor("rgba(0, 122, 255, 0.14)"); !ok || c.B != 255 || c.A != 36 {
		t.Fatalf("rgba: %v %v", c, ok)
	}
	if c, ok := parseColor("#fff"); !ok || c.R != 255 {
		t.Fatalf("hex: %v", c)
	}
	if parsePx("12px") != 12 || parsePx("0.5rem") != 8 {
		t.Fatal("px")
	}
}
