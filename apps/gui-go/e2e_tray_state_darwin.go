//go:build e2e && darwin

package main

import (
	"reflect"
	"unsafe"

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

// trayEnableOpenMenu makes Wails' own SystemTray.OpenMenu usable in the e2e build. OpenMenu starts with `if s.menu == nil { return }`, and
// SystemTray.SetMenu does not set that field once the tray runs (it only updates the implementation's menu), so after initTray the call is a
// no-op. This fills the private field with the Menu the tray already shows (the same *Menu the implementation holds), e2e build only. It
// reports whether it had to. Production code is untouched; production right click does not use OpenMenu (see the tracking document).
func trayEnableOpenMenu(tray *application.SystemTray, menu *application.Menu) bool {
	f := reflect.ValueOf(tray).Elem().FieldByName("menu")
	if !f.IsValid() || !f.IsNil() {
		return false
	}
	reflect.NewAt(f.Type(), unsafe.Pointer(f.UnsafeAddr())).Elem().Set(reflect.ValueOf(menu))
	return true
}
