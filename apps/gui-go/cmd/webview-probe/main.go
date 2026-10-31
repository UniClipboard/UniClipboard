//go:build windows && e2e

// A disposable WebView2 target for CDP capability characterization only.
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"strconv"

	"github.com/wailsapp/wails/v3/pkg/application"
)

func main() {
	port, err := strconv.Atoi(os.Args[1])
	if err != nil || port < 1 || port > 65535 {
		panic("invalid private CDP port")
	}
	data, err := os.MkdirTemp("", "uc-gui-go-webview-probe-")
	if err != nil {
		panic(err)
	}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html")
		fmt.Fprint(w, `<!doctype html><button onclick="this.textContent='native action confirmed'">isolated WebView2</button>`)
	}))
	defer server.Close()
	app := application.New(application.Options{
		Name:    "UniClipboard isolated WebView probe",
		Windows: application.WindowsOptions{WebviewUserDataPath: data, AdditionalBrowserArgs: []string{fmt.Sprintf("--remote-debugging-port=%d", port), "--remote-debugging-address=127.0.0.1"}},
	})
	// A visible control is required for normal pointer actionability and frame delivery.
	// This executable runs only on a disposable Windows runner.
	window := app.Window.NewWithOptions(application.WebviewWindowOptions{Title: "Isolated WebView2 probe", URL: server.URL, Width: 600, Height: 400})
	json.NewEncoder(os.Stdout).Encode(map[string]any{"url": server.URL, "pid": os.Getpid(), "isolatedData": true, "dataDir": data})
	go func() { io.Copy(io.Discard, os.Stdin); window.Close() }()
	if err := app.Run(); err != nil {
		panic(err)
	}
	// The parent removes the private data only after this host exits and releases COM references.
	json.NewEncoder(os.Stdout).Encode(map[string]any{"phase": "cleanup", "windowClosed": true})
}
