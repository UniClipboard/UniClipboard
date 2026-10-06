package main

/*
#cgo CFLAGS: -x objective-c -fobjc-arc
#cgo LDFLAGS: -framework Foundation
#import <Foundation/Foundation.h>
#include <stdlib.h>

extern void goBackgroundActivityFired(int mainThread, int deferred, char *queue);

// startBackgroundActivity schedules a repeating NSBackgroundActivityScheduler, Apple's API for energy-efficient
// periodic work: the system runs it even while App Nap has suspended the process's timers. It hands the
// callback thread and the system's "defer" hint to Go and completes at once (the check runs on the scheduler
// goroutine, not inside this block). The returned handle owns the scheduler until stopBackgroundActivity.
static void *startBackgroundActivity(const char *identifier, double interval, double tolerance) {
	NSBackgroundActivityScheduler *scheduler = [[NSBackgroundActivityScheduler alloc] initWithIdentifier:[NSString stringWithUTF8String:identifier]];
	scheduler.repeats = YES;
	scheduler.interval = interval;
	scheduler.tolerance = tolerance;
	__weak NSBackgroundActivityScheduler *weak = scheduler; // the scheduler retains the block
	[scheduler scheduleWithBlock:^(NSBackgroundActivityCompletionHandler completion) {
		// shouldDefer is the system asking for the work to wait (low power, thermal pressure): honour it and
		// let the system reschedule instead of waking the app.
		int deferred = weak.shouldDefer;
		goBackgroundActivityFired([NSThread isMainThread], deferred, (char *)dispatch_queue_get_label(DISPATCH_CURRENT_QUEUE_LABEL));
		completion(deferred ? NSBackgroundActivityResultDeferred : NSBackgroundActivityResultFinished);
	}];
	return (void *)CFBridgingRetain(scheduler);
}

static void stopBackgroundActivity(void *handle) {
	NSBackgroundActivityScheduler *scheduler = (NSBackgroundActivityScheduler *)CFBridgingRelease(handle);
	[scheduler invalidate];
}
*/
import "C"

import (
	"log"
	"sync/atomic"
	"time"
	"unsafe"
)

// backgroundActivityIdentifier is the Tauri shell's identifier; Apple asks for a stable reverse-domain name.
const backgroundActivityIdentifier = "app.uniclipboard.update-check"

// backgroundActivityTolerance is the Tauri shell's 10% window the system may use to coalesce the activity with
// other wakeups.
const backgroundActivityTolerance = 0.1

// onBackgroundActivity is the callback of the one scheduler a process owns; it is cleared on stop so a block
// already queued by the system cannot reach a finished scheduler loop.
var onBackgroundActivity atomic.Pointer[func()]

//export goBackgroundActivityFired
func goBackgroundActivityFired(mainThread, deferred C.int, queue *C.char) {
	log.Printf("update scheduler: background activity callback (mainThread=%v, deferred=%v, queue=%s)", mainThread != 0, deferred != 0, C.GoString(queue))
	if deferred != 0 {
		return
	}
	if fire := onBackgroundActivity.Load(); fire != nil {
		(*fire)()
	}
}

// startBackgroundActivity makes the system call fire every interval (within the tolerance) while the process
// lives, App Nap or not. The returned function invalidates the scheduler.
func startBackgroundActivity(interval time.Duration, fire func()) (stop func()) {
	onBackgroundActivity.Store(&fire)
	id := C.CString(backgroundActivityIdentifier)
	defer C.free(unsafe.Pointer(id))
	handle := C.startBackgroundActivity(id, C.double(interval.Seconds()), C.double(interval.Seconds()*backgroundActivityTolerance))
	log.Printf("update scheduler: background activity scheduled every %s", interval)
	return func() {
		onBackgroundActivity.Store(nil)
		C.stopBackgroundActivity(handle)
		log.Printf("update scheduler: background activity stopped")
	}
}
