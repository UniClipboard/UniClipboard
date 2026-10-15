package hostlog

import (
	"bytes"
	"encoding/json"
	"log"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func fixedClock(t time.Time) func() time.Time { return func() time.Time { return t } }

func readRecords(t *testing.T, path string) []map[string]any {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var out []map[string]any
	for _, line := range strings.Split(strings.TrimRight(string(data), "\n"), "\n") {
		var record map[string]any
		if err := json.Unmarshal([]byte(line), &record); err != nil {
			t.Fatalf("not a JSON line %q: %v", line, err)
		}
		out = append(out, record)
	}
	return out
}

func install(t *testing.T, dir string, now time.Time, stderr *bytes.Buffer) *Logger {
	t.Helper()
	logger, err := Open(Options{Dir: dir, Stem: "uniclipboard-gui", Now: fixedClock(now), Stderr: stderr})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = logger.Close() })
	return logger
}

func TestRecordsCarryTheRunIdentityInTheDaemonFormat(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 10, 15, 8, 30, 0, 0, time.UTC)
	logger := install(t, dir, now, &bytes.Buffer{})

	logger.Info("hello %s", "world")
	logger.Error("boom")

	records := readRecords(t, filepath.Join(dir, "uniclipboard-gui.json.2026-10-15"))
	if len(records) != 2 {
		t.Fatalf("records = %v", records)
	}
	first := records[0]
	if first["level"] != "INFO" || first["message"] != "hello world" || first["target"] != "go.host" {
		t.Fatalf("first = %v", first)
	}
	if first["timestamp"] != "2026-10-15T08:30:00.000Z" {
		t.Fatalf("timestamp = %v", first["timestamp"])
	}
	if int(first["pid"].(float64)) != os.Getpid() {
		t.Fatalf("pid = %v", first["pid"])
	}
	runID, _ := first["run_id"].(string)
	if len(runID) != 36 || records[1]["run_id"] != runID || records[1]["level"] != "ERROR" {
		t.Fatalf("run identity: %v", records)
	}
}

func TestStandardLogOutputIsRedirectedAndStillEchoed(t *testing.T) {
	dir := t.TempDir()
	var stderr bytes.Buffer
	logger := install(t, dir, time.Date(2026, 10, 15, 1, 0, 0, 0, time.UTC), &stderr)
	previous := log.Writer()
	t.Cleanup(func() { log.SetOutput(previous) })

	logger.RedirectStandardLog()
	log.Print("from the standard logger\nsecond line")

	records := readRecords(t, filepath.Join(dir, "uniclipboard-gui.json.2026-10-15"))
	if len(records) != 1 || !strings.HasPrefix(records[0]["message"].(string), "from the standard logger") {
		t.Fatalf("records = %v", records)
	}
	if !strings.Contains(stderr.String(), "from the standard logger") {
		t.Fatalf("stderr lost the line: %q", stderr.String())
	}
}

func TestTheFileFollowsTheCalendarDay(t *testing.T) {
	dir := t.TempDir()
	now := time.Date(2026, 10, 15, 23, 59, 59, 0, time.UTC)
	clock := now
	logger, err := Open(Options{Dir: dir, Stem: "uniclipboard-gui", Now: func() time.Time { return clock }, Stderr: &bytes.Buffer{}})
	if err != nil {
		t.Fatal(err)
	}
	defer logger.Close()

	logger.Info("before midnight")
	clock = clock.Add(2 * time.Second)
	logger.Info("after midnight")

	for day, message := range map[string]string{"2026-10-15": "before midnight", "2026-10-16": "after midnight"} {
		records := readRecords(t, filepath.Join(dir, "uniclipboard-gui.json."+day))
		if len(records) != 1 || records[0]["message"] != message {
			t.Fatalf("%s = %v", day, records)
		}
	}
}

func TestOldFilesOfThisRoleAreRemovedAndOthersKept(t *testing.T) {
	dir := t.TempDir()
	for _, name := range []string{
		"uniclipboard-gui.json.2026-10-01", // older than 7 days
		"uniclipboard-gui.json.2026-10-09", // inside the window
		"uniclipboard-daemon.json.2026-10-01",
		"engine.2026-10-01.jsonl",
		"notes.txt",
	} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte("x\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}

	install(t, dir, time.Date(2026, 10, 15, 8, 0, 0, 0, time.UTC), &bytes.Buffer{})

	if _, err := os.Stat(filepath.Join(dir, "uniclipboard-gui.json.2026-10-01")); !os.IsNotExist(err) {
		t.Fatalf("expired own file kept: %v", err)
	}
	for _, kept := range []string{"uniclipboard-gui.json.2026-10-09", "uniclipboard-daemon.json.2026-10-01", "engine.2026-10-01.jsonl", "notes.txt"} {
		if _, err := os.Stat(filepath.Join(dir, kept)); err != nil {
			t.Fatalf("%s: %v", kept, err)
		}
	}
}

func TestAnOversizedMessageIsBounded(t *testing.T) {
	dir := t.TempDir()
	logger := install(t, dir, time.Date(2026, 10, 15, 8, 0, 0, 0, time.UTC), &bytes.Buffer{})

	logger.Info("%s", strings.Repeat("a", 100000))

	message := readRecords(t, filepath.Join(dir, "uniclipboard-gui.json.2026-10-15"))[0]["message"].(string)
	if len(message) > MaxMessageBytes+16 {
		t.Fatalf("message length %d", len(message))
	}
}

func TestAnUnusableDirectoryIsReportedNotFatal(t *testing.T) {
	blocker := filepath.Join(t.TempDir(), "file")
	if err := os.WriteFile(blocker, []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := Open(Options{Dir: filepath.Join(blocker, "logs"), Stem: "uniclipboard-gui"}); err == nil {
		t.Fatal("expected an error")
	}
}
