//go:build e2e && darwin

package main

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework Cocoa
#import <Cocoa/Cocoa.h>

// postSystemWake posts the very notification macOS posts after sleep, on the center Wails observes. It does
// not sleep the machine: it only feeds Wails' own observer -> Mac.ApplicationDidWake -> Common.SystemDidWake
// chain, which is the code under test (real sleep and wake of the host stays unverified).
static void postSystemWake(void) {
    NSWorkspace *workspace = [NSWorkspace sharedWorkspace];
    [[workspace notificationCenter] postNotificationName:NSWorkspaceDidWakeNotification object:workspace];
}
*/
import "C"

func postSystemWake() error {
	C.postSystemWake()
	return nil
}
