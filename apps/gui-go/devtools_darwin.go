//go:build darwin && private_mac_apis && !production && !release

package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa -framework WebKit
#import <Cocoa/Cocoa.h>
#import <WebKit/WebKit.h>

// Wails' NativeWindow is an NSWindow with a webView property.
@interface NSWindow (UniClipboardDevelopmentTools)
@property (readonly) WKWebView *webView;
@end

static bool enableLocalInspector(void *handle) {
    NSWindow *window = (__bridge NSWindow *)handle;
    if (window == nil) return false;
    @try {
        [window.webView.configuration.preferences setValue:@YES forKey:@"developerExtrasEnabled"];
        return true;
    } @catch (NSException *exception) {
        return false;
    }
}
*/
import "C"

import (
	"github.com/wailsapp/wails/v3/pkg/application"
	"log"
)

// Wails beta.28 enables remote inspection on macOS 13.3+, but skips the preference
// needed by its local inspector. Set it on this development WebView before opening.
func openDevelopmentTools(window application.Window) {
	application.InvokeSync(func() {
		if !bool(C.enableLocalInspector(window.NativeWindow())) {
			log.Print("failed to enable the local development inspector")
		}
	})
	window.OpenDevTools()
}
