package pluginproc

import (
	"context"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"github.com/uniclipboard/unic/plugin"
)

func envMap(entries []string, fold bool) map[string]string {
	out := make(map[string]string, len(entries))
	for _, kv := range entries {
		k, v, ok := strings.Cut(kv, "=")
		if !ok {
			continue
		}
		if fold {
			k = strings.ToUpper(k)
		}
		out[k] = v
	}
	return out
}

func TestBuildEnvWindows(t *testing.T) {
	parent := []string{
		`SystemRoot=C:\Windows`,
		`windir=C:\Windows`,
		`Temp=C:\Users\alice\AppData\Local\Temp`,
		`tmp=C:\Users\alice\AppData\Local\Temp`,
		`https_proxy=http://127.0.0.1:7890`,
		`NO_PROXY=localhost,127.0.0.1`,
		`USERPROFILE=C:\Users\alice`,
		`APPDATA=C:\Users\alice\AppData\Roaming`,
		`GITHUB_TOKEN=ghp_secret`,
		`UNIC_HOME=C:\Users\alice\.unic`,
	}
	extra := []string{
		`UNIC_TEST_PLUGIN=serve`,
		`HTTPS_PROXY=http://127.0.0.1:8888`,
		`UNIC_PLUGIN_PROTOCOL=999`,
		`UNIC_PLUGIN_DIR=C:\wrong`,
	}
	got := envMap(buildEnv("windows", parent, extra, `C:\data\unic.wormhole`, `C:\fallback`), true)

	want := map[string]string{
		"SYSTEMROOT":           `C:\Windows`,
		"WINDIR":               `C:\Windows`,
		"TEMP":                 `C:\Users\alice\AppData\Local\Temp`,
		"TMP":                  `C:\Users\alice\AppData\Local\Temp`,
		"HTTPS_PROXY":          `http://127.0.0.1:8888`,
		"NO_PROXY":             `localhost,127.0.0.1`,
		"UNIC_TEST_PLUGIN":     `serve`,
		"UNIC_PLUGIN_PROTOCOL": `1`,
		"UNIC_PLUGIN_DIR":      `C:\data\unic.wormhole`,
	}
	for k, w := range want {
		if got[k] != w {
			t.Errorf("%s = %q, want %q", k, got[k], w)
		}
	}
	for _, forbidden := range []string{"USERPROFILE", "APPDATA", "GITHUB_TOKEN", "UNIC_HOME"} {
		if _, exists := got[forbidden]; exists {
			t.Errorf("unexpected inherited variable %s=%q", forbidden, got[forbidden])
		}
	}
}

func TestBuildEnvWindowsTempFallback(t *testing.T) {
	parent := []string{
		`SystemRoot=C:\Windows`,
		`TEMP=`,
		`USERPROFILE=C:\Users\alice`,
	}
	got := envMap(buildEnv("windows", parent, nil, `C:\data\unic.wormhole`, `C:\Users\alice\AppData\Local\Temp`), true)
	if got["TEMP"] != `C:\Users\alice\AppData\Local\Temp` || got["TMP"] != `C:\Users\alice\AppData\Local\Temp` {
		t.Fatalf("TEMP=%q TMP=%q, want fallback temp dir", got["TEMP"], got["TMP"])
	}
	if _, exists := got["USERPROFILE"]; exists {
		t.Fatal("USERPROFILE must not be inherited")
	}
}

func TestBuildEnvUnix(t *testing.T) {
	parent := []string{
		"TMPDIR=/var/folders/custom/tmp",
		"SSL_CERT_FILE=/etc/ssl/custom.pem",
		"SSL_CERT_DIR=/etc/ssl/certs",
		"HTTPS_PROXY=http://127.0.0.1:7890",
		"http_proxy=http://127.0.0.1:7890",
		"no_proxy=localhost",
		"HOME=/Users/alice",
		"PATH=/usr/bin:/bin",
		"AWS_SECRET_ACCESS_KEY=secret",
		"UNIC_HOME=/Users/alice/.unic",
	}
	extra := []string{
		"UNIC_TEST_PLUGIN=serve",
		"UNIC_PLUGIN_PROTOCOL=0",
	}
	got := envMap(buildEnv("darwin", parent, extra, "/data/unic.wormhole", "/ignored"), false)

	want := map[string]string{
		"TMPDIR":               "/var/folders/custom/tmp",
		"SSL_CERT_FILE":        "/etc/ssl/custom.pem",
		"SSL_CERT_DIR":         "/etc/ssl/certs",
		"HTTPS_PROXY":          "http://127.0.0.1:7890",
		"http_proxy":           "http://127.0.0.1:7890",
		"no_proxy":             "localhost",
		"UNIC_TEST_PLUGIN":     "serve",
		"UNIC_PLUGIN_PROTOCOL": "1",
		"UNIC_PLUGIN_DIR":      "/data/unic.wormhole",
	}
	for k, w := range want {
		if got[k] != w {
			t.Errorf("%s = %q, want %q", k, got[k], w)
		}
	}
	for _, forbidden := range []string{"HOME", "PATH", "AWS_SECRET_ACCESS_KEY", "UNIC_HOME", "TEMP", "TMP"} {
		if _, exists := got[forbidden]; exists {
			t.Errorf("unexpected inherited variable %s=%q", forbidden, got[forbidden])
		}
	}
}

func TestSubprocessMinimalEnvironment(t *testing.T) {
	customTmp := t.TempDir()
	if runtime.GOOS == "windows" {
		t.Setenv("TEMP", customTmp)
		t.Setenv("TMP", customTmp)
	} else {
		t.Setenv("TMPDIR", customTmp)
	}
	t.Setenv("HTTPS_PROXY", "http://127.0.0.1:17890")
	t.Setenv("UNIC_TEST_UNRELATED_SECRET", "must-not-leak")

	p, _ := start(t, exeSpec(t, "serve"), Options{}, nil)
	ac := &fakeAction{id: "probe-env", item: newProc("probe").item}
	if err := p.(plugin.Actor).Run(context.Background(), ac); err != nil {
		t.Fatalf("probe-env action failed: %v", err)
	}
	got := envMap(strings.Split(ac.text, "\n"), false)
	wantDir, err := filepath.EvalSymlinks(customTmp)
	if err != nil {
		wantDir = customTmp
	}
	gotDir, err := filepath.EvalSymlinks(got["temp"])
	if err != nil {
		gotDir = got["temp"]
	}
	if gotDir != wantDir {
		t.Fatalf("subprocess temp dir = %q (resolved %q), want %q", got["temp"], gotDir, wantDir)
	}
	if got["https_proxy"] != "http://127.0.0.1:17890" {
		t.Fatalf("subprocess HTTPS_PROXY = %q, want http://127.0.0.1:17890", got["https_proxy"])
	}
	if got["secret"] != "" {
		t.Fatalf("unrelated host secret leaked into plugin subprocess: %q", got["secret"])
	}
}
