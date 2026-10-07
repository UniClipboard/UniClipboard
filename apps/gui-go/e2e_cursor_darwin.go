//go:build e2e && darwin

package main

/*
#cgo LDFLAGS: -framework CoreGraphics
#include <CoreGraphics/CoreGraphics.h>

static void warpCursor(double x, double y) {
	CGWarpMouseCursorPosition(CGPointMake(x, y));
	CGAssociateMouseAndMouseCursorPosition(1);
}
*/
import "C"

// warpCursor moves the real pointer; coordinates are top-left based, as Wails screens.
func warpCursor(x, y float64) { C.warpCursor(C.double(x), C.double(y)) }
