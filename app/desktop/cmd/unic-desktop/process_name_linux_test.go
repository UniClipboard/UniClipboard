//go:build linux

package main

import (
	"fmt"
	"os"
	"runtime"
	"strings"
	"testing"

	"golang.org/x/sys/unix"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopprocess"
)

func TestSetRoleProcessNameLinux(t *testing.T) {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	commPath := fmt.Sprintf("/proc/self/task/%d/comm", unix.Gettid())
	origRaw, err := os.ReadFile(commPath)
	if err != nil {
		t.Fatalf("read %s: %v", commPath, err)
	}
	orig := strings.TrimSpace(string(origRaw))
	defer setProcessName(orig)

	exeBefore, err := os.Executable()
	if err != nil {
		t.Fatalf("os.Executable before: %v", err)
	}

	for _, role := range []desktopprocess.Role{desktopprocess.Tray, desktopprocess.GUI, desktopprocess.QuickPanel} {
		setRoleProcessName(role)
		gotRaw, err := os.ReadFile(commPath)
		if err != nil {
			t.Fatalf("read %s for %q: %v", commPath, role, err)
		}
		got := strings.TrimSpace(string(gotRaw))
		want := roleProcessName(role)
		if got != want {
			t.Fatalf("role %q comm = %q, want %q", role, got, want)
		}
	}

	exeAfter, err := os.Executable()
	if err != nil {
		t.Fatalf("os.Executable after: %v", err)
	}
	if exeAfter != exeBefore {
		t.Fatalf("os.Executable changed from %q to %q", exeBefore, exeAfter)
	}
}
