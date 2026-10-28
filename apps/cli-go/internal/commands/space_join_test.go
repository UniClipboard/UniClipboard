package commands

import "testing"

func TestNormalizeInvitationCode(t *testing.T) {
	cases := []struct{ name, input, want string }{
		{"short", "012345", "012-345"},
		{"formatted", " 012 - 345\n", "012-345"},
		{"incomplete", "01234", "01234"},
		{"legacy invalid", "ab-cd", "ABCD"},
		{"full", " \tucspace1_AbC-012_xyz\n", "ucspace1_AbC-012_xyz"},
		{"opaque", "ucspace1_Ab C-012", "ucspace1_Ab C-012"},
		{"case sensitive prefix", "UCSPACE1_AbC-012", "UCSPACE1_ABC012"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := normalizeInvitationCode(tc.input); got != tc.want {
				t.Fatal("normalization changed invitation semantics")
			}
		})
	}
}
