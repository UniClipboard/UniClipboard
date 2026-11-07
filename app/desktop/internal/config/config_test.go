package config

import (
	"context"
	"os"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"
)

func TestParseDefaultsAndValues(t *testing.T) {
	c, err := Parse([]byte(""))
	if err != nil || c != Default() {
		t.Fatalf("empty file: %+v, %v", c, err)
	}
	c, err = Parse([]byte(`
device_name = "laptop"
[limits]
max_file_bytes = "512MB"
max_total_bytes = 1048576
[sync]
images = false
`))
	if err != nil {
		t.Fatal(err)
	}
	if c.DeviceName != "laptop" || c.Limits.MaxFileBytes != 512<<20 || c.Limits.MaxTotalBytes != 1<<20 ||
		c.Sync.Images || !c.Sync.Text || !c.Sync.Enabled {
		t.Errorf("unexpected: %+v", c)
	}
}

func TestParseRejects(t *testing.T) {
	for name, in := range map[string]string{
		"unknown key":   `devce_name = "x"`,
		"unknown table": "[nope]\na = 1",
		"syntax":        `device_name = `,
		"bad url":       `rendezvous_url = "ftp://x"`,
		"relative":      `inbox = "rel/dir"`,
		"over hard cap": "[limits]\nmax_file_bytes = \"3GiB\"",
		"zero":          "[limits]\nmax_total_bytes = 0",
		"bad size":      "[limits]\nmax_file_bytes = \"lots\"",
		"wrong type":    "[sync]\ntext = \"yes\"",
		"no history":    "[history]\nmax_items = 0",
		"history size":  "[history]\nmax_bytes = \"9GiB\"",
		"negative ttl":  "[history]\ndefault_ttl_days = -1",
	} {
		if _, err := Parse([]byte(in)); err == nil {
			t.Errorf("%s: expected an error", name)
		}
	}
}

func TestOpenCreatesTemplate(t *testing.T) {
	p := filepath.Join(t.TempDir(), "config.toml")
	m, err := Open(p, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	if m.Current() != Default() {
		t.Errorf("template should be a no-op, got %+v", m.Current())
	}
	if b, _ := os.ReadFile(p); !strings.Contains(string(b), "device_name") {
		t.Error("template not written")
	}
	// An existing file is never overwritten.
	if err := os.WriteFile(p, []byte(`device_name = "keep"`), 0o600); err != nil {
		t.Fatal(err)
	}
	if m, err = Open(p, t.Logf); err != nil || m.Current().DeviceName != "keep" {
		t.Errorf("existing file: %+v, %v", m.Current(), err)
	}
	// An invalid file at startup is an error: there is nothing to fall back to.
	if err := os.WriteFile(p, []byte(`oops`), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err = Open(p, t.Logf); err == nil {
		t.Error("expected an error for an invalid file at startup")
	}
}

func TestWatchAppliesSavesAndKeepsLastGood(t *testing.T) {
	p := filepath.Join(t.TempDir(), "config.toml")
	m, err := Open(p, t.Logf)
	if err != nil {
		t.Fatal(err)
	}
	var calls atomic.Int32
	m.OnChange = func(_, _ Config, _ error) { calls.Add(1) }
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() { _ = m.Watch(ctx) }()
	time.Sleep(100 * time.Millisecond)

	// Save the way many editors do: write a temp file, rename it over.
	save := func(content string) {
		tmp := p + ".tmp"
		if err := os.WriteFile(tmp, []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
		if err := os.Rename(tmp, p); err != nil {
			t.Fatal(err)
		}
	}
	waitFor := func(what string, ok func() bool) {
		t.Helper()
		for i := 0; i < 100; i++ {
			if ok() {
				return
			}
			time.Sleep(20 * time.Millisecond)
		}
		t.Fatalf("timed out waiting for %s", what)
	}

	save(`device_name = "one"`)
	waitFor("first edit", func() bool { return m.Current().DeviceName == "one" })

	save(`device_name = "two" oops`)
	waitFor("error recorded", func() bool { return m.State().Error != "" })
	if m.Current().DeviceName != "one" {
		t.Errorf("bad edit replaced the last good config: %+v", m.Current())
	}

	save(`device_name = "three"`)
	waitFor("fix applied", func() bool { return m.Current().DeviceName == "three" && m.State().Error == "" })

	// Bursts of events are coalesced.
	before := calls.Load()
	for i := 0; i < 5; i++ {
		if err := os.WriteFile(p, []byte(`device_name = "burst"`), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	waitFor("burst", func() bool { return m.Current().DeviceName == "burst" })
	time.Sleep(300 * time.Millisecond)
	if n := calls.Load() - before; n > 2 {
		t.Errorf("burst of writes caused %d reloads", n)
	}

	// Inbox changes wait for a restart.
	save("inbox = \"" + t.TempDir() + "\"")
	waitFor("inbox edit", func() bool { return len(m.State().RestartRequired) == 1 })
	// So do history limits.
	save("inbox = \"\"\n[history]\nmax_items = 5\ndefault_ttl_days = 7")
	waitFor("history edit", func() bool {
		rr := m.State().RestartRequired
		return len(rr) == 1 && rr[0] == "history"
	})
	if o := m.Current().HistoryOptions(); o.MaxItems != 5 || o.DefaultTTL != 7*24*time.Hour || o.MaxBytes != 4<<30 {
		t.Errorf("options = %+v", o)
	}
}

func TestParseSize(t *testing.T) {
	for in, want := range map[string]int64{"5": 5, "1KB": 1 << 10, "2 MiB": 2 << 20, "1gb": 1 << 30} {
		if got, err := ParseSize(in); err != nil || got != want {
			t.Errorf("ParseSize(%q) = %d, %v; want %d", in, got, err, want)
		}
	}
	for _, in := range []string{"", "MB", "-1", "1.5GB", "9999999999999GB"} {
		if _, err := ParseSize(in); err == nil {
			t.Errorf("ParseSize(%q): expected an error", in)
		}
	}
}
