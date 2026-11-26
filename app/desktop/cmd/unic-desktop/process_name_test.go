package main

import (
	"bytes"
	"encoding/binary"
	"os"
	"strings"
	"testing"
	"unicode/utf16"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
)

func TestRoleProcessName(t *testing.T) {
	seen := map[string]desktopprocess.Role{}
	roles := []desktopprocess.Role{
		desktopprocess.Tray,
		desktopprocess.GUI,
		desktopprocess.QuickPanel,
	}
	for _, role := range roles {
		name := roleProcessName(role)
		if name == "" {
			t.Fatalf("role %q has empty process name", role)
		}
		// Linux PR_SET_NAME truncates at 15 bytes (TASK_COMM_LEN - 1).
		if len(name) > 15 {
			t.Fatalf("role %q process name %q exceeds 15-byte Linux limit (%d)", role, name, len(name))
		}
		if strings.ContainsRune(name, '\x00') {
			t.Fatalf("role %q process name %q contains NUL", role, name)
		}
		if prev, dup := seen[name]; dup {
			t.Fatalf("roles %q and %q share process name %q", prev, role, name)
		}
		seen[name] = role
	}
	if got := roleProcessName(desktopprocess.Role("unknown")); got != "" {
		t.Fatalf("unknown role process name = %q, want empty", got)
	}
}

func utf16LE(s string) []byte {
	units := utf16.Encode([]rune(s))
	out := make([]byte, len(units)*2)
	for i, u := range units {
		binary.LittleEndian.PutUint16(out[i*2:], u)
	}
	return out
}

func TestWindowsResourcesEmbedVersionInfo(t *testing.T) {
	sig := []byte{0xBD, 0x04, 0xEF, 0xFE} // VS_FIXEDFILEINFO signature 0xFEEF04BD
	marker := utf16LE("VS_VERSION_INFO")
	expected := [][2]string{
		{"CompanyName", "UniClipboard"},
		{"FileDescription", "Unic"},
		{"InternalName", "unic-desktop"},
		{"OriginalFilename", "unic-desktop.exe"},
		{"ProductName", "Unic"},
	}
	for _, arch := range []string{"amd64", "arm64"} {
		path := "icon_windows_" + arch + ".syso"
		data, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		idx := bytes.Index(data, marker)
		if idx < 0 {
			t.Fatalf("%s missing VS_VERSION_INFO", path)
		}
		sigIdx := bytes.Index(data[idx:], sig)
		if sigIdx < 0 {
			t.Fatalf("%s missing VS_FIXEDFILEINFO signature", path)
		}
		versionBlock := data[idx+sigIdx:]
		for _, pair := range expected {
			key := append(utf16LE(pair[0]), 0, 0)
			val := append(utf16LE(pair[1]), 0, 0)
			kpos := bytes.Index(versionBlock, key)
			if kpos < 0 {
				t.Fatalf("%s missing VERSIONINFO key %q", path, pair[0])
			}
			windowEnd := kpos + len(key) + 8 + len(val)
			if windowEnd > len(versionBlock) {
				windowEnd = len(versionBlock)
			}
			if !bytes.Contains(versionBlock[kpos+len(key):windowEnd], val) {
				t.Fatalf("%s VERSIONINFO key %q missing value %q", path, pair[0], pair[1])
			}
		}
	}
}
