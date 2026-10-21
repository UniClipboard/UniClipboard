package main

/*
#cgo CFLAGS: -x objective-c -fobjc-arc
#cgo LDFLAGS: -framework Foundation
#import <Foundation/Foundation.h>
#include <stdlib.h>

// firstPreferredLanguage returns the first of the user's preferred languages, or NULL. The caller frees it.
static char *firstPreferredLanguage(void) {
	NSString *language = [NSLocale preferredLanguages].firstObject;
	return language == nil ? NULL : strdup(language.UTF8String);
}
*/
import "C"

import "unsafe"

// systemLanguage is the display language the system prefers, as the quick panel reads it
// (apps/quick-panel/src/platform/macos/mod.rs): the first of NSLocale's preferred languages.
func systemLanguage() string {
	language := C.firstPreferredLanguage()
	if language == nil {
		return ""
	}
	defer C.free(unsafe.Pointer(language))
	return C.GoString(language)
}
