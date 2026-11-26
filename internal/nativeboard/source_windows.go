//go:build windows

package nativeboard

import (
	"bytes"
	"image"
	"image/png"
	"path/filepath"
	"strings"
	"unsafe"

	"golang.org/x/sys/windows"

	"github.com/uniclipboard/unic/sourceapp"
)

var sourceUser32 = windows.NewLazySystemDLL("user32.dll")
var clipboardOwner = sourceUser32.NewProc("GetClipboardOwner")
var sourceClipboardSequence = sourceUser32.NewProc("GetClipboardSequenceNumber")
var sourceWindowPID = sourceUser32.NewProc("GetWindowThreadProcessId")

func ClipboardOwnerRevision() uint64 { n, _, _ := sourceClipboardSequence.Call(); return uint64(n) }
func ClipboardSource() (uint64, *sourceapp.Application) {
	revision := ClipboardOwnerRevision()
	owner, _, _ := clipboardOwner.Call()
	if owner == 0 {
		return revision, nil
	}
	var pid uint32
	_, _, _ = sourceWindowPID.Call(owner, uintptr(unsafe.Pointer(&pid)))
	if pid == 0 {
		return revision, nil
	}
	process, err := windows.OpenProcess(windows.PROCESS_QUERY_LIMITED_INFORMATION, false, pid)
	if err != nil {
		return revision, nil
	}
	defer func() { _ = windows.CloseHandle(process) }()
	var buf [32768]uint16
	size := uint32(len(buf))
	if windows.QueryFullProcessImageName(process, 0, &buf[0], &size) != nil {
		return revision, nil
	}
	path := windows.UTF16ToString(buf[:size])
	id := strings.ToLower(filepath.Base(path))
	a := &sourceapp.Application{Platform: "windows", Identifier: id, Name: filepath.Base(path), Attribution: "clipboard_owner"}
	if a.Validate() != nil {
		return revision, nil
	}
	a.IconPNG = CachedIcon("windows:"+path, func() []byte { return executableIcon(path) })
	return revision, a
}

// Extract the executable's small icon into a fixed-size top-down DIB.
// The executable path stays local and never enters the metadata.
func executableIcon(path string) []byte {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil
	}
	shell := windows.NewLazySystemDLL("shell32.dll")
	var icon uintptr
	n, _, _ := shell.NewProc("ExtractIconExW").Call(uintptr(unsafe.Pointer(p)), 0, 0, uintptr(unsafe.Pointer(&icon)), 1)
	if n == 0 || icon == 0 {
		return nil
	}
	defer func() { _, _, _ = sourceUser32.NewProc("DestroyIcon").Call(icon) }()
	gdi := windows.NewLazySystemDLL("gdi32.dll")
	dc, _, _ := gdi.NewProc("CreateCompatibleDC").Call(0)
	if dc == 0 {
		return nil
	}
	defer func() { _, _, _ = gdi.NewProc("DeleteDC").Call(dc) }()
	info := struct {
		Size                   uint32
		Width, Height          int32
		Planes, BitCount       uint16
		Compression, SizeImage uint32
		X, Y                   int32
		Used, Important        uint32
	}{Size: 40, Width: 64, Height: -64, Planes: 1, BitCount: 32}
	var bits unsafe.Pointer
	bitmap, _, _ := gdi.NewProc("CreateDIBSection").Call(dc, uintptr(unsafe.Pointer(&info)), 0, uintptr(unsafe.Pointer(&bits)), 0, 0)
	if bitmap == 0 || bits == nil {
		return nil
	}
	defer func() { _, _, _ = gdi.NewProc("DeleteObject").Call(bitmap) }()
	old, _, _ := gdi.NewProc("SelectObject").Call(dc, bitmap)
	defer func() { _, _, _ = gdi.NewProc("SelectObject").Call(dc, old) }()
	raw := unsafe.Slice((*byte)(bits), 64*64*4)
	clear(raw)
	ok, _, _ := sourceUser32.NewProc("DrawIconEx").Call(dc, 0, 0, icon, 64, 64, 0, 0, 3)
	if ok == 0 {
		return nil
	}
	// DrawIconEx produces premultiplied BGRA. image.RGBA preserves alpha.
	img := image.NewRGBA(image.Rect(0, 0, 64, 64))
	for i := 0; i < len(raw); i += 4 {
		img.Pix[i], img.Pix[i+1], img.Pix[i+2], img.Pix[i+3] = raw[i+2], raw[i+1], raw[i], raw[i+3]
	}
	var b bytes.Buffer
	if png.Encode(&b, img) != nil {
		return nil
	}
	return b.Bytes()
}
