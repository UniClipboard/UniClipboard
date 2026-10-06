package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>

// Cursor position in points with a top-left origin on the primary display,
// the coordinate space Wails uses for screens and window positions.
static int cursorTopLeft(double *x, double *y) {
	NSArray<NSScreen *> *screens = [NSScreen screens];
	if (screens.count == 0) return 0;
	NSPoint p = [NSEvent mouseLocation];
	*x = p.x;
	*y = screens[0].frame.size.height - p.y;
	return 1;
}
*/
import "C"

// cursorPosition reports the global mouse position, or false when unavailable.
func cursorPosition() (x, y float64, ok bool) {
	var cx, cy C.double
	if C.cursorTopLeft(&cx, &cy) == 0 {
		return 0, 0, false
	}
	return float64(cx), float64(cy), true
}
