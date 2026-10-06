package main

/*
#cgo LDFLAGS: -framework ApplicationServices
#include <ApplicationServices/ApplicationServices.h>
*/
import "C"

// accessibilityTrusted reports whether this app may observe and post keyboard events,
// which the modifier double-tap trigger and auto-paste need.
func accessibilityTrusted() bool { return C.AXIsProcessTrusted() != 0 }
