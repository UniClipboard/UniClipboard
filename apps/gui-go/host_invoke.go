package main

import (
	"context"
	"encoding/json"
	"fmt"
	"sort"
	"time"
)

// InvokeResult is the envelope returned to the WebView. The frontend adapter
// rethrows Error unchanged so typed command errors keep their Tauri shape.
type InvokeResult struct {
	Ok    bool `json:"ok"`
	Data  any  `json:"data,omitempty"`
	Error any  `json:"error,omitempty"`
}

// commandError mirrors the Rust `CommandError` wire shape.
type commandError struct {
	Code    string `json:"code"`
	Message string `json:"message"`
}

func (e commandError) Error() string { return e.Code + ": " + e.Message }

func internalError(err error) error {
	return commandError{Code: "InternalError", Message: err.Error()}
}

// codeError is a typed error serialised as `{ "code": ... }` only.
type codeError struct {
	Code string `json:"code"`
}

func (e codeError) Error() string { return e.Code }

type commandArgs map[string]json.RawMessage

func (a commandArgs) decode(key string, out any) error {
	raw, ok := a[key]
	if !ok {
		return commandError{Code: "ValidationError", Message: "missing argument " + key}
	}
	if err := json.Unmarshal(raw, out); err != nil {
		return commandError{Code: "ValidationError", Message: "invalid argument " + key}
	}
	return nil
}

type commandFunc func(ctx context.Context, h *HostService, args commandArgs) (any, error)

// commands is the single routing table for the Tauri command surface.
// Anything absent here is reported as unsupported by Invoke and listed by
// UnsupportedCommands, which the coverage check compares with the generated
// bindings.
var commands = map[string]commandFunc{}

func register(table map[string]commandFunc) {
	for name, fn := range table {
		commands[name] = fn
	}
}

// Invoke executes one host command on behalf of the shared React frontend.
func (h *HostService) Invoke(name string, args map[string]json.RawMessage) InvokeResult {
	fn, ok := commands[name]
	if !ok {
		return InvokeResult{Error: commandError{Code: "InternalError", Message: fmt.Sprintf("command %s is not available in the Go host", name)}}
	}
	ctx, cancel := context.WithTimeout(context.Background(), commandTimeout(name))
	defer cancel()
	data, err := fn(ctx, h, args)
	if err != nil {
		return InvokeResult{Error: wireError(err)}
	}
	return InvokeResult{Ok: true, Data: data}
}

// RegisteredCommands lists implemented command names for coverage reporting.
func RegisteredCommands() []string {
	names := make([]string, 0, len(commands))
	for name := range commands {
		names = append(names, name)
	}
	sort.Strings(names)
	return names
}

// wireError keeps typed errors as-is and wraps anything else so the WebView
// always receives an object with a stable `code`.
func wireError(err error) any {
	switch typed := err.(type) {
	case commandError, codeError, stringError, configError:
		return typed
	default:
		return internalError(err)
	}
}

// channel resolves a frontend Channel argument, which crosses the bridge as
// `{ "__channel": id }`, into a sender that delivers messages on that id.
func (a commandArgs) channel(key string) (func(*HostService, any), error) {
	var ref struct {
		ID string `json:"__channel"`
	}
	if err := a.decode(key, &ref); err != nil || ref.ID == "" {
		return nil, commandError{Code: "ValidationError", Message: "invalid channel argument " + key}
	}
	return func(h *HostService, message any) { h.emit("channel://"+ref.ID, message) }, nil
}

// commandTimeout gives long-running update commands room; others fail fast.
func commandTimeout(name string) time.Duration {
	switch name {
	case "download_update", "install_update":
		return 30 * time.Minute
	}
	return 30 * time.Second
}
