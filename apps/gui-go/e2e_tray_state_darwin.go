//go:build e2e && darwin

package main

import (
	"reflect"

	"github.com/wailsapp/wails/v3/pkg/application"
)

// trayNativeState reports, read-only through reflection, which parts of the Wails system tray exist: the Go-side menu and click handlers
// and the macOS implementation's NSStatusItem and NSMenu pointers (the fields openMenu and the pre-click callback test for nil). It exists
// to tell "the open request reached a tray with no NSMenu" from "the open ran and tracking did not start"; it changes nothing.
func trayNativeState(tray *application.SystemTray) map[string]any {
	state := map[string]any{}
	v := reflect.ValueOf(tray).Elem()
	nilOf := func(name string) any {
		f := v.FieldByName(name)
		if !f.IsValid() {
			return "no such field"
		}
		return f.IsNil()
	}
	state["menuNil"] = nilOf("menu")
	state["implNil"] = nilOf("impl")
	state["clickHandlerNil"] = nilOf("clickHandler")
	state["rightClickHandlerNil"] = nilOf("rightClickHandler")
	if impl := v.FieldByName("impl"); impl.IsValid() && !impl.IsNil() {
		if s := impl.Elem(); s.Kind() == reflect.Ptr && !s.IsNil() {
			s = s.Elem()
			for _, name := range []string{"nsStatusItem", "nsMenu"} {
				if f := s.FieldByName(name); f.IsValid() {
					state[name+"Nil"] = f.Pointer() == 0
				} else {
					state[name+"Nil"] = "no such field"
				}
			}
		}
	}
	return state
}
