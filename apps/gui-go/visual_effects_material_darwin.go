package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>

// Wails beta.28 exposes the NSWindow handle but no runtime backdrop setter.
// Its translucent backdrop is a sibling of the WKWebView in contentView.
static void uc_set_gui_effects(void *handle, int low) {
    NSWindow *window = (__bridge NSWindow *)handle;
    for (NSView *view in window.contentView.subviews) {
        if ([view isKindOfClass:[NSVisualEffectView class]]) {
            NSVisualEffectView *effect = (NSVisualEffectView *)view;
            effect.state = low ? NSVisualEffectStateInactive : NSVisualEffectStateFollowsWindowActiveState;
            effect.hidden = low != 0;
        }
    }
}
*/
import "C"

import "github.com/wailsapp/wails/v3/pkg/application"

func applyGUIEffectsMaterial(w *application.WebviewWindow, low bool) {
	value := C.int(0)
	if low {
		value = 1
	}
	if handle := w.NativeWindow(); handle != nil {
		C.uc_set_gui_effects(handle, value)
	}
}
