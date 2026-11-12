//go:build darwin && cgo

package board

/*
#cgo CFLAGS: -x objective-c
#cgo LDFLAGS: -framework AppKit
#import <AppKit/AppKit.h>
static long long unicPasteboardChangeCount(void) {
 @autoreleasepool { return (long long)[[NSPasteboard generalPasteboard] changeCount]; }
}
*/
import "C"
import "strconv"

func clipboardCopyID() string { return strconv.FormatInt(int64(C.unicPasteboardChangeCount()), 10) }
