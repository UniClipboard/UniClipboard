// Command installer_contract checks, without a Windows host, the parts of the Windows in-place update contract
// that are pure data: the NSIS argument escaping (the golden table of tauri-plugin-updater 2.10.1's
// `it_escapes_correctly_for_nsis`), the installer command line, and installer extraction from the two payload
// shapes the release feed can carry (a bare `*-setup.exe` and the `*.nsis.zip` wrapper). It writes
// `installer-contract-assertions.json` into the directory given as the only argument and exits non-zero on a
// failed assertion.
package main

import (
	"archive/zip"
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
)

type assertion struct {
	Name   string `json:"name"`
	Passed bool   `json:"passed"`
	Detail string `json:"detail,omitempty"`
}

var results []assertion

func check(name string, ok bool, detail string) {
	results = append(results, assertion{name, ok, detail})
}

func makeZip(files map[string][]byte) []byte {
	var buf bytes.Buffer
	zw := zip.NewWriter(&buf)
	for name, data := range files {
		w, _ := zw.Create(name)
		_, _ = w.Write(data)
	}
	_ = zw.Close()
	return buf.Bytes()
}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: installer_contract <outdir>")
		os.Exit(2)
	}
	// Golden table copied from tauri-plugin-updater 2.10.1 src/updater.rs (`it_escapes_correctly_for_nsis`).
	golden := [][2]string{
		{"something", "something"},
		{"--flag", "--flag"},
		{"--empty=", "--empty="},
		{"--arg=value", "--arg=value"},
		{"some space", `"some space"`},
		{"--arg value", `"--arg value"`},
		{"--arg=unwrapped space", `"--arg=unwrapped space"`},
		{`--arg="wrapped"`, `--arg=\"wrapped\"`},
		{`--arg="wrapped space"`, `"--arg=\"wrapped space\""`},
		{`--arg=midword"wrapped space"`, `"--arg=midword\"wrapped space\""`},
		{"", `""`},
	}
	for _, c := range golden {
		got := update.EscapeNSISArg(c[0])
		check("escape "+fmt.Sprintf("%q", c[0]), got == c[1], fmt.Sprintf("got %s want %s", got, c[1]))
	}
	// `/` must be quoted so NSIS does not read it as its own switch (the std-lib rules alone would not).
	check("escape slash is quoted", update.EscapeNSISArg("/quick") == `"/quick"`, update.EscapeNSISArg("/quick"))
	check("escape trailing backslashes doubled", update.EscapeNSISArg(`a b\`) == `"a b\\"`, update.EscapeNSISArg(`a b\`))

	args := update.NSISArgs([]string{"--autostart"})
	check("installer args, autostart launch", args == "/P /R /UPDATE /ARGS --autostart", args)
	args = update.NSISArgs(nil)
	check("installer args, no arguments", args == "/P /R /UPDATE /ARGS", args)

	exe := append([]byte("MZ"), bytes.Repeat([]byte{0}, 64)...)
	got, err := update.ExtractInstaller(exe)
	check("bare setup.exe is accepted as is", err == nil && bytes.Equal(got, exe), fmt.Sprint(err))
	got, err = update.ExtractInstaller(makeZip(map[string][]byte{"UniClipboard_1.2.0_x64-setup.exe": exe}))
	check("nsis.zip yields its exe", err == nil && bytes.Equal(got, exe), fmt.Sprint(err))
	_, err = update.ExtractInstaller(makeZip(map[string][]byte{"readme.txt": []byte("hi")}))
	check("zip without an exe is refused", errors.Is(err, update.ErrNoInstaller), fmt.Sprint(err))
	_, err = update.ExtractInstaller(makeZip(map[string][]byte{"fake.exe": []byte("not a pe image")}))
	check("zip whose exe is not a PE image is refused", errors.Is(err, update.ErrNoInstaller), fmt.Sprint(err))
	_, err = update.ExtractInstaller([]byte("a macOS tarball, say"))
	check("a payload that is neither is refused", errors.Is(err, update.ErrNoInstaller), fmt.Sprint(err))
	_, err = update.ExtractInstaller(nil)
	check("an empty payload is refused", errors.Is(err, update.ErrNoInstaller), fmt.Sprint(err))

	failed := 0
	for _, r := range results {
		if !r.Passed {
			failed++
			fmt.Fprintf(os.Stderr, "FAIL %s: %s\n", r.Name, r.Detail)
		}
	}
	raw, _ := json.MarshalIndent(map[string]any{"failed": failed, "total": len(results), "assertions": results}, "", "  ")
	if err := os.WriteFile(filepath.Join(os.Args[1], "installer-contract-assertions.json"), raw, 0o644); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
	fmt.Printf("%d/%d assertions passed\n", len(results)-failed, len(results))
	if failed > 0 {
		os.Exit(1)
	}
}
