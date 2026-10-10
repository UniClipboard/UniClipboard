//go:build e2e

package main

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
)

// e2eCommand is one row of the generated e2eCommandTable (e2e_invoke_table.go).
type e2eCommand struct {
	Method string
	Params []string
}

// e2eInvokeCommand calls a host command by its wire name with named JSON arguments, the way the control file's
// `invoke` verb and the E2E scripts address one. It calls the real exported HostService method by reflection, so no
// separate command table exists; the parameter names come from the generated table. A missing or null argument decodes
// to the zero value, exactly like a Wails call. The result is `{ok, data, error}` with the error in its wire form.
func e2eInvokeCommand(h *HostService, command string, args map[string]json.RawMessage) map[string]any {
	row, ok := e2eCommandTable[command]
	if !ok {
		return map[string]any{"command": command, "ok": false, "error": map[string]any{"code": "InternalError", "message": "unknown command " + command}}
	}
	method := reflect.ValueOf(h).MethodByName(row.Method)
	typ := method.Type()
	in := make([]reflect.Value, 0, typ.NumIn())
	ctx, cancel := context.WithTimeout(context.Background(), commandTimeout(command))
	defer cancel()
	next := 0
	for i := 0; i < typ.NumIn(); i++ {
		if typ.In(i) == reflect.TypeFor[context.Context]() {
			in = append(in, reflect.ValueOf(ctx))
			continue
		}
		value := reflect.New(typ.In(i))
		if next < len(row.Params) {
			if raw, present := args[row.Params[next]]; present {
				if err := json.Unmarshal(raw, value.Interface()); err != nil {
					return map[string]any{"command": command, "ok": false, "error": map[string]any{"code": "ValidationError", "message": fmt.Sprintf("argument %s: %v", row.Params[next], err)}}
				}
			}
		}
		next++
		in = append(in, value.Elem())
	}
	out := method.Call(in)
	result := map[string]any{"command": command, "ok": true}
	for _, value := range out {
		if err, isErr := value.Interface().(error); isErr && value.Type() == reflect.TypeFor[error]() {
			if err != nil {
				result["ok"] = false
				var wire any
				_ = json.Unmarshal(hostapi.Marshal(err), &wire)
				result["error"] = wire
			}
			continue
		}
		result["data"] = value.Interface()
	}
	return result
}
