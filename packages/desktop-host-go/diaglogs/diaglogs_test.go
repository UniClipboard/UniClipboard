package diaglogs

import (
	"archive/zip"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"slices"
	"testing"
	"time"
)

type roleFixture struct {
	Prefix   string   `json:"prefix"`
	Suffix   string   `json:"suffix"`
	Positive []string `json:"positive"`
	Negative []string `json:"negative"`
}

func loadFixture(t *testing.T) map[string]roleFixture {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "log-source-patterns.json"))
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		Comment string                 `json:"_comment"`
		Roles   map[string]roleFixture `json:"roles"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	if fixture.Comment == "" {
		t.Fatal("fixture must keep its _comment")
	}
	return fixture.Roles
}

func TestSourcePatternsMatchSharedFixture(t *testing.T) {
	roles := loadFixture(t)
	if len(roles) != len(sources) {
		t.Fatalf("fixture has %d roles, code has %d", len(roles), len(sources))
	}
	for _, src := range sources {
		fx, ok := roles[src.role]
		if !ok {
			t.Fatalf("role %q missing from the fixture", src.role)
		}
		if fx.Prefix != src.prefix || fx.Suffix != src.suffix {
			t.Errorf("role %q: fixture %q..%q, code %q..%q", src.role, fx.Prefix, fx.Suffix, src.prefix, src.suffix)
		}
		for _, name := range fx.Positive {
			if got := roleOf(name); got != src.role {
				t.Errorf("roleOf(%q) = %q, want %q", name, got, src.role)
			}
		}
		for _, name := range fx.Negative {
			if got := roleOf(name); got != "" {
				t.Errorf("roleOf(%q) = %q, want no role", name, got)
			}
		}
	}
}

func touch(t *testing.T, dir string, names ...string) {
	t.Helper()
	for _, name := range names {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(name+"\n"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func readArchive(t *testing.T, path string) map[string][]byte {
	t.Helper()
	r, err := zip.OpenReader(path)
	if err != nil {
		t.Fatal(err)
	}
	defer r.Close()
	files := map[string][]byte{}
	for _, f := range r.File {
		rc, err := f.Open()
		if err != nil {
			t.Fatal(err)
		}
		data, err := io.ReadAll(rc)
		rc.Close()
		if err != nil {
			t.Fatal(err)
		}
		files[f.Name] = data
	}
	return files
}

type decodedManifest struct {
	Collection struct {
		IncludedFiles   []string `json:"includedFiles"`
		UnreadableFiles []string `json:"unreadableFiles"`
		MissingSources  []struct {
			Source string `json:"source"`
			Reason string `json:"reason"`
		} `json:"missingSources"`
	} `json:"collection"`
}

func export(t *testing.T, logs string) (map[string][]byte, decodedManifest) {
	t.Helper()
	dest := filepath.Join(t.TempDir(), "out.zip")
	if _, err := ExportStartup(logs, dest, nil); err != nil {
		t.Fatal(err)
	}
	files := readArchive(t, dest)
	var m decodedManifest
	if err := json.Unmarshal(files["manifest.json"], &m); err != nil {
		t.Fatal(err)
	}
	return files, m
}

func TestExportIncludesQuickPanelLogs(t *testing.T) {
	logs := t.TempDir()
	touch(t, logs, "uniclipboard-quick-panel.json.2026-10-14", "uniclipboard-gui.json.2026-10-14")
	files, m := export(t, logs)
	if _, ok := files["logs/uniclipboard-quick-panel.json.2026-10-14"]; !ok {
		t.Fatalf("quick-panel log missing from the archive; included = %v", m.Collection.IncludedFiles)
	}
}

func TestExportPropagatesReadDirError(t *testing.T) {
	notADir := filepath.Join(t.TempDir(), "logs")
	touch(t, filepath.Dir(notADir), "logs")
	_, err := ExportStartup(notADir, filepath.Join(t.TempDir(), "out.zip"), nil)
	if err == nil || errors.Is(err, ErrNoLogs) {
		t.Fatalf("err = %v, want the directory read error", err)
	}
}

func TestExportMissingDirectoryReportsNoLogs(t *testing.T) {
	_, err := ExportStartup(filepath.Join(t.TempDir(), "absent"), filepath.Join(t.TempDir(), "out.zip"), nil)
	if !errors.Is(err, ErrNoLogs) {
		t.Fatalf("err = %v, want ErrNoLogs", err)
	}
}

func TestManifestReportsMissingSources(t *testing.T) {
	logs := t.TempDir()
	touch(t, logs, "uniclipboard-gui.json.2026-10-14", "engine.2026-10-14.jsonl")
	_, m := export(t, logs)
	got := map[string]string{}
	for _, s := range m.Collection.MissingSources {
		got[s.Source] = s.Reason
	}
	want := map[string]string{"daemon": "missing", "cli": "missing", "quick-panel": "missing"}
	if len(got) != len(want) {
		t.Fatalf("missingSources = %v, want %v", got, want)
	}
	for k, v := range want {
		if got[k] != v {
			t.Fatalf("missingSources = %v, want %v", got, want)
		}
	}
}

func TestManifestReportsUnreadableSource(t *testing.T) {
	if runtime.GOOS == "windows" || os.Geteuid() == 0 {
		t.Skip("file permissions cannot make a file unreadable here")
	}
	logs := t.TempDir()
	touch(t, logs, "uniclipboard-gui.json.2026-10-14", "uniclipboard-daemon.json.2026-10-14")
	if err := os.Chmod(filepath.Join(logs, "uniclipboard-daemon.json.2026-10-14"), 0); err != nil {
		t.Fatal(err)
	}
	_, m := export(t, logs)
	if !slices.Contains(m.Collection.UnreadableFiles, "uniclipboard-daemon.json.2026-10-14") {
		t.Fatalf("unreadableFiles = %v", m.Collection.UnreadableFiles)
	}
	var reason string
	for _, s := range m.Collection.MissingSources {
		if s.Source == "daemon" {
			reason = s.Reason
		}
	}
	if reason != "unreadable" {
		t.Fatalf("daemon reason = %q, want unreadable; missingSources = %v", reason, m.Collection.MissingSources)
	}
}

func TestManifestSnapshot(t *testing.T) {
	logs := t.TempDir()
	touch(t, logs, "uniclipboard-gui.json.2026-10-14", "uniclipboard-quick-panel.json.2026-10-14", "engine.2026-10-14.jsonl")
	env := environment{
		now: func() time.Time { return time.Date(2026, 10, 14, 8, 30, 0, 0, time.UTC) },
		host: func(now time.Time) hostBlock {
			return hostBlock{AppVersion: "1.2.0-alpha.5", OS: "linux", Arch: "amd64", Profile: "dev", Portable: false, TimezoneOffsetMinutes: 480}
		},
	}
	dest := filepath.Join(t.TempDir(), "out.zip")
	if _, err := exportStartup(env, logs, dest, json.RawMessage(`{"phase":"ready"}`)); err != nil {
		t.Fatal(err)
	}
	got := readArchive(t, dest)["manifest.json"]
	want, err := os.ReadFile(filepath.Join("testdata", "manifest-offline.golden.json"))
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(want) {
		t.Fatalf("manifest differs from testdata/manifest-offline.golden.json\n--- got ---\n%s\n--- want ---\n%s", got, want)
	}
}

func loadUndatedFiles(t *testing.T) []string {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "log-source-patterns.json"))
	if err != nil {
		t.Fatal(err)
	}
	var fixture struct {
		UndatedFiles []string `json:"undatedFiles"`
	}
	if err := json.Unmarshal(data, &fixture); err != nil {
		t.Fatal(err)
	}
	if len(fixture.UndatedFiles) == 0 {
		t.Fatal("fixture must list undatedFiles")
	}
	return fixture.UndatedFiles
}

func TestUndatedFilesFromFixtureAreExported(t *testing.T) {
	for _, name := range loadUndatedFiles(t) {
		logs := t.TempDir()
		touch(t, logs, name, "uniclipboard-gui.json.2026-10-14")
		files, m := export(t, logs)
		if _, ok := files["logs/"+name]; !ok || !slices.Contains(m.Collection.IncludedFiles, name) {
			t.Errorf("%s not exported; included = %v", name, m.Collection.IncludedFiles)
		}
		if roleOf(name) != "" {
			t.Errorf("%s must not belong to a dated role", name)
		}
	}
}

func TestStartupFailureRecordAloneIsExportContent(t *testing.T) {
	logs := t.TempDir()
	touch(t, logs, "daemon-startup-failure.json")
	files, m := export(t, logs)
	if _, ok := files["logs/daemon-startup-failure.json"]; !ok {
		t.Fatalf("record missing from archive; included = %v", m.Collection.IncludedFiles)
	}
	if !slices.Contains(m.Collection.IncludedFiles, "daemon-startup-failure.json") {
		t.Fatalf("included = %v", m.Collection.IncludedFiles)
	}
}

func TestStartupFailureRecordIsCappedAt4096Bytes(t *testing.T) {
	logs := t.TempDir()
	big := make([]byte, 10000)
	for i := range big {
		big[i] = 'x'
	}
	if err := os.WriteFile(filepath.Join(logs, "daemon-startup-failure.json"), big, 0o600); err != nil {
		t.Fatal(err)
	}
	files, _ := export(t, logs)
	if got := len(files["logs/daemon-startup-failure.json"]); got != 4096 {
		t.Fatalf("exported %d bytes, want 4096", got)
	}
}

func TestStartupFailureRecordThatIsNotARegularFileIsUnreadable(t *testing.T) {
	logs := t.TempDir()
	if err := os.Mkdir(filepath.Join(logs, "daemon-startup-failure.json"), 0o700); err != nil {
		t.Fatal(err)
	}
	touch(t, logs, "uniclipboard-gui.json.2026-10-14")
	files, m := export(t, logs)
	if !slices.Contains(m.Collection.UnreadableFiles, "daemon-startup-failure.json") {
		t.Fatalf("unreadableFiles = %v", m.Collection.UnreadableFiles)
	}
	if _, ok := files["logs/daemon-startup-failure.json"]; ok {
		t.Fatal("a directory must not be archived")
	}
}

func TestExportReturnsTheManifestMissingSources(t *testing.T) {
	logs := t.TempDir()
	touch(t, logs, "uniclipboard-gui.json.2026-10-14", "engine.2026-10-14.jsonl")
	dest := filepath.Join(t.TempDir(), "out.zip")
	report, err := ExportStartup(logs, dest, nil)
	if err != nil {
		t.Fatal(err)
	}
	want := []MissingSource{{"daemon", "missing"}, {"cli", "missing"}, {"quick-panel", "missing"}}
	if !slices.Equal(report.MissingSources, want) {
		t.Fatalf("report.MissingSources = %v, want %v", report.MissingSources, want)
	}
	var m decodedManifest
	if err := json.Unmarshal(readArchive(t, dest)["manifest.json"], &m); err != nil {
		t.Fatal(err)
	}
	if len(m.Collection.MissingSources) != len(want) {
		t.Fatalf("manifest = %v, report = %v", m.Collection.MissingSources, report.MissingSources)
	}
}

func TestFailedExportReturnsNoGaps(t *testing.T) {
	report, err := ExportStartup(filepath.Join(t.TempDir(), "absent"), filepath.Join(t.TempDir(), "out.zip"), nil)
	if !errors.Is(err, ErrNoLogs) || report.MissingSources != nil {
		t.Fatalf("report = %v, err = %v", report, err)
	}
}
