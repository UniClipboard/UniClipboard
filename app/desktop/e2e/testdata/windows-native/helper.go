// Command helper speaks the existing Desktop automation and local HTTP protocols.
// It contains no clipboard code or Desktop business implementation.
package main

import (
	"context"
	"debug/buildinfo"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
func run() error {
	flags := flag.NewFlagSet("windows-native-helper", flag.ContinueOnError)
	endpoint := flags.String("endpoint", "", "existing Unix socket endpoint")
	method := flags.String("method", "windows", "automation method or HTTP method")
	params := flags.String("params", "null", "JSON params/body")
	path := flags.String("path", "/v1/service", "local API path")
	if len(os.Args) < 2 {
		return fmt.Errorf("usage: helper build-info|rpc|api [flags]")
	}
	command := os.Args[1]
	if command == "canonical" {
		if len(os.Args) != 3 {
			return fmt.Errorf("canonical requires a path")
		}
		path, err := filepath.Abs(os.Args[2])
		if err != nil {
			return err
		}
		path, err = filepath.EvalSymlinks(path)
		if err != nil {
			return err
		}
		_, err = fmt.Fprintln(os.Stdout, path)
		return err
	}
	if command == "build-info" {
		if len(os.Args) != 3 {
			return fmt.Errorf("build-info requires a binary path")
		}
		info, err := buildinfo.ReadFile(os.Args[2])
		if err != nil {
			return err
		}
		return json.NewEncoder(os.Stdout).Encode(info)
	}
	if err := flags.Parse(os.Args[2:]); err != nil {
		return err
	}
	if !json.Valid([]byte(*params)) {
		return fmt.Errorf("invalid JSON params")
	}
	ctx, cancel := context.WithTimeout(context.Background(), 15*time.Second)
	defer cancel()
	if command == "api" {
		client := &http.Client{Transport: &http.Transport{DialContext: func(ctx context.Context, _, _ string) (net.Conn, error) {
			return (&net.Dialer{}).DialContext(ctx, "unix", *endpoint)
		}}}
		request, err := http.NewRequestWithContext(ctx, *method, "http://local"+*path, strings.NewReader(*params))
		if err != nil {
			return err
		}
		request.Header.Set("Content-Type", "application/json")
		response, err := client.Do(request)
		if err != nil {
			return err
		}
		defer func() { _ = response.Body.Close() }()
		body, err := io.ReadAll(io.LimitReader(response.Body, 4<<20))
		if err != nil {
			return err
		}
		if response.StatusCode < 200 || response.StatusCode >= 300 {
			return fmt.Errorf("HTTP %d: %s", response.StatusCode, body)
		}
		_, err = os.Stdout.Write(body)
		return err
	}
	if command != "rpc" {
		return fmt.Errorf("unknown command")
	}
	conn, err := (&net.Dialer{}).DialContext(ctx, "unix", *endpoint)
	if err != nil {
		return err
	}
	defer func() { _ = conn.Close() }()
	_ = conn.SetDeadline(time.Now().Add(15 * time.Second))
	if err = json.NewEncoder(conn).Encode(map[string]any{"id": 1, "method": *method, "params": json.RawMessage(*params)}); err != nil {
		return err
	}
	var response struct {
		Result json.RawMessage `json:"result"`
		Error  json.RawMessage `json:"error"`
	}
	if err = json.NewDecoder(conn).Decode(&response); err != nil {
		return err
	}
	if len(response.Error) > 0 && string(response.Error) != "null" {
		return fmt.Errorf("automation: %s", response.Error)
	}
	_, err = os.Stdout.Write(append(response.Result, '\n'))
	return err
}
