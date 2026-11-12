package board

import (
	"golang.org/x/sys/windows"
	"strconv"
)

var clipboardSequence = windows.NewLazySystemDLL("user32.dll").NewProc("GetClipboardSequenceNumber")

func clipboardCopyID() string {
	v, _, _ := clipboardSequence.Call()
	return strconv.FormatUint(uint64(v), 10)
}
