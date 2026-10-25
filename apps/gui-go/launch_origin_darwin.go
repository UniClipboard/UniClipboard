package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>
#import <CoreServices/CoreServices.h>

static BOOL ucLoginLaunch = NO;
static id ucLaunchObserver;
static void ucObserveLaunchOrigin(void) {
    ucLaunchObserver = [[NSNotificationCenter defaultCenter]
        addObserverForName:NSApplicationDidFinishLaunchingNotification
        object:nil queue:nil usingBlock:^(NSNotification *notification) {
            NSAppleEventDescriptor *event = [[NSAppleEventManager sharedAppleEventManager] currentAppleEvent];
            ucLoginLaunch = [[event paramDescriptorForKeyword:keyAEPropData] enumCodeValue] == keyAELaunchedAsLogInItem;
        }];
}
static int ucWasLoginLaunch(void) { return ucLoginLaunch ? 1 : 0; }
*/
import "C"

import "github.com/wailsapp/wails/v3/pkg/application"

// Wails beta.28 SMAppService drops Arguments and exposes registration status,
// not launch origin. Capture AppKit's login Apple event synchronously while the
// did-finish notification is delivered, before an asynchronous Go hook loses it.
func (h *HostService) installLaunchOriginObserver() {
	C.ucObserveLaunchOrigin()
}

func platformAutomaticLaunch() bool {
	var automatic bool
	// The main queue finishes delivering the notification before this read.
	application.InvokeSync(func() { automatic = C.ucWasLoginLaunch() != 0 })
	return automatic
}
