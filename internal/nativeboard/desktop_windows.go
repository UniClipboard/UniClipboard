package nativeboard

import (
	"bytes"
	"context"
	"fmt"
	"os"
	"runtime"
	"strconv"
	"strings"
	"time"
	"unsafe"

	"golang.design/x/clipboard"
	"golang.org/x/sys/windows"

	"github.com/uniclipboard/unic/internal/clip"
)

var desktopFormats = []desktopFormat{
	{mime: clip.Text, format: clipboard.FmtText},
	{mime: clip.HTML, format: clipboard.Register("HTML Format"), alternate: clipboard.Register(clip.HTML), decode: decodeClipboardHTML, encode: encodeClipboardHTML},
	{mime: clip.PNG, format: clipboard.Register(clip.PNG), alternate: clipboard.FmtImage},
}

var (
	getProcessWindowStation = sourceUser32.NewProc("GetProcessWindowStation")
	getUserObjectInfo       = sourceUser32.NewProc("GetUserObjectInformationW")
	createClipboardWindow   = sourceUser32.NewProc("CreateWindowExW")
	destroyClipboardWindow  = sourceUser32.NewProc("DestroyWindow")
	addClipboardListener    = sourceUser32.NewProc("AddClipboardFormatListener")
	removeClipboardListener = sourceUser32.NewProc("RemoveClipboardFormatListener")
	getClipboardMessage     = sourceUser32.NewProc("GetMessageW")
	postClipboardMessage    = sourceUser32.NewProc("PostMessageW")
	openClipboard           = sourceUser32.NewProc("OpenClipboard")
	closeClipboard          = sourceUser32.NewProc("CloseClipboard")
)

func checkDesktopSession() error {
	var session uint32
	if err := windows.ProcessIdToSessionId(uint32(os.Getpid()), &session); err != nil || session == 0 {
		return ErrDesktopClipboardUnsupported
	}
	station, _, _ := getProcessWindowStation.Call()
	// USEROBJECTFLAGS contains two BOOLs and a DWORD; WSF_VISIBLE is bit 0.
	var flags [3]uint32
	var needed uint32
	ok, _, _ := getUserObjectInfo.Call(station, 1, uintptr(unsafe.Pointer(&flags[0])),
		unsafe.Sizeof(flags), uintptr(unsafe.Pointer(&needed)))
	if ok == 0 || flags[2]&1 == 0 {
		return ErrDesktopClipboardUnsupported
	}
	return nil
}

func (*Board) ClipboardGeneration() string {
	revision := ClipboardOwnerRevision()
	if revision == 0 {
		return ""
	}
	return strconv.FormatUint(revision, 10)
}

// ClipboardAvailable checks and releases the system lock before dispatch of a
// native paste. The sequence is checked again by the caller after the probe.
func (*Board) ClipboardAvailable() bool {
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	deadline := time.Now().Add(200 * time.Millisecond)
	for {
		ok, _, _ := openClipboard.Call(0)
		if ok != 0 {
			closed, _, _ := closeClipboard.Call()
			return closed != 0
		}
		if time.Now().After(deadline) {
			return false
		}
		time.Sleep(10 * time.Millisecond)
	}
}

type clipboardMSG struct {
	window  uintptr
	message uint32
	wParam  uintptr
	lParam  uintptr
	time    uint32
	x, y    int32
	private uint32
}

const (
	clipboardUpdateMessage = 0x031d
	closeListenerMessage   = 0x0010
)

// ClipboardChanges sends a notification without opening the clipboard. The
// adapter compares the native generation before reading any representation.
func (*Board) ClipboardChanges(ctx context.Context) <-chan struct{} {
	changes := make(chan struct{}, 1)
	go func() {
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		defer close(changes)
		name, _ := windows.UTF16PtrFromString("STATIC")
		hwnd, _, _ := createClipboardWindow.Call(0, uintptr(unsafe.Pointer(name)), 0, 0,
			0, 0, 0, 0, ^uintptr(2), 0, 0, 0) // HWND_MESSAGE
		if hwnd == 0 {
			return
		}
		defer func() { _, _, _ = destroyClipboardWindow.Call(hwnd) }()
		ok, _, _ := addClipboardListener.Call(hwnd)
		if ok == 0 {
			return
		}
		defer func() { _, _, _ = removeClipboardListener.Call(hwnd) }()
		done := make(chan struct{})
		stop := make(chan struct{})
		defer func() { close(stop); <-done }()
		go func() {
			defer close(done)
			select {
			case <-ctx.Done():
				_, _, _ = postClipboardMessage.Call(hwnd, closeListenerMessage, 0, 0)
			case <-stop:
			}
		}()
		for ctx.Err() == nil {
			var message clipboardMSG
			ok, _, _ = getClipboardMessage.Call(uintptr(unsafe.Pointer(&message)), hwnd, 0, 0)
			if int32(ok) <= 0 || message.message == closeListenerMessage {
				return
			}
			if message.message == clipboardUpdateMessage {
				select {
				case changes <- struct{}{}:
				default:
				}
			}
		}
	}()
	return changes
}

func decodeClipboardHTML(data []byte) []byte {
	data = bytes.TrimRight(data, "\x00")
	header := string(data[:min(len(data), 4096)])
	offset := func(key string) int {
		_, value, ok := strings.Cut(header, key+":")
		if !ok {
			return -1
		}
		line, _, _ := strings.Cut(value, "\n")
		n, err := strconv.Atoi(strings.TrimSpace(line))
		if err != nil {
			return -1
		}
		return n
	}
	start, end := offset("StartFragment"), offset("EndFragment")
	if start < 0 || end < start || end > len(data) {
		return nil
	}
	return data[start:end]
}

func encodeClipboardHTML(data []byte) []byte {
	const header = "Version:0.9\r\nStartHTML:%010d\r\nEndHTML:%010d\r\nStartFragment:%010d\r\nEndFragment:%010d\r\n"
	const prefix = "<html><body><!--StartFragment-->"
	const suffix = "<!--EndFragment--></body></html>"
	html := len(fmt.Sprintf(header, 0, 0, 0, 0))
	start := html + len(prefix)
	end := start + len(data)
	return fmt.Appendf(nil, header+"%s%s%s%c", html, end+len(suffix), start, end, prefix, data, suffix, 0)
}

func platformClipboard() *Board { return nil }
