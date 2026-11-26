//go:build windows

package quickpanel

import (
	"math"
	"runtime"
	"sync"
	"syscall"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	getCursorPos        = user32.NewProc("GetCursorPos")
	getWindowRect       = user32.NewProc("GetWindowRect")
	getClassNameW       = user32.NewProc("GetClassNameW")
	getGUIThreadInfo    = user32.NewProc("GetGUIThreadInfo")
	mapWindowPoints     = user32.NewProc("MapWindowPoints")
	enumDisplayMonitors = user32.NewProc("EnumDisplayMonitors")
	getMonitorInfoW     = user32.NewProc("GetMonitorInfoW")
	monitorFromRect     = user32.NewProc("MonitorFromRect")

	oleacc                    = windows.NewLazySystemDLL("oleacc.dll")
	accessibleObjectFromWnd   = oleacc.NewProc("AccessibleObjectFromWindow")
	ole32                     = windows.NewLazySystemDLL("ole32.dll")
	coInitializeEx            = ole32.NewProc("CoInitializeEx")
	coCreateInstance          = ole32.NewProc("CoCreateInstance")
	coUninitialize            = ole32.NewProc("CoUninitialize")
	oleaut32                  = windows.NewLazySystemDLL("oleaut32.dll")
	safeArrayGetLBound        = oleaut32.NewProc("SafeArrayGetLBound")
	safeArrayGetUBound        = oleaut32.NewProc("SafeArrayGetUBound")
	safeArrayAccessData       = oleaut32.NewProc("SafeArrayAccessData")
	safeArrayUnaccessData     = oleaut32.NewProc("SafeArrayUnaccessData")
	safeArrayDestroy          = oleaut32.NewProc("SafeArrayDestroy")
	shcore                    = windows.NewLazySystemDLL("shcore.dll")
	getDpiForMonitor          = shcore.NewProc("GetDpiForMonitor")
)

const (
	objidCaret              = uintptr(uint32(0xFFFFFFF8))
	vtI4                    = 3
	childIDShelf            = 0
	coinitMultithreaded     = 0x0
	clsctxInprocServer      = 0x1
	monitorDefaultToNearest = 2
	mdtEffectiveDPI         = 0

	uiaTextPatternID         = 10014
	uiaTextPattern2ID        = 10024
	uiaComboBoxControlTypeID = 50003
	uiaEditControlTypeID     = 50004
	uiaTextUnitCharacter     = 0

	// COM vtable indices verified against Microsoft Windows SDK / windows-rs metadata.
	vtblRelease                        = 2
	vtblQueryInterface                 = 0
	vtblIAccessibleAccLocation         = 22 // IDispatch(7) + 15
	vtblUIAGetFocusedElement           = 8  // IUnknown(3) + 5
	vtblUIA2PutConnectionTimeout       = 61 // IUIAutomation(58) + 3
	vtblUIA2PutTransactionTimeout      = 63 // IUIAutomation(58) + 5
	vtblUIAElemGetCurrentPatternAs     = 14 // IUnknown(3) + 11
	vtblUIAElemGetCurrentControlType   = 21 // IUnknown(3) + 18
	vtblUIAElemGetCurrentBoundingRect  = 43 // IUnknown(3) + 40
	vtblUIATextPatternGetSelection     = 5  // IUnknown(3) + 2
	vtblUIATextPattern2GetCaretRange   = 10 // IUIAutomationTextPattern(9) + 1
	vtblUIATextRangeArrayGetLength     = 3  // IUnknown(3) + 0
	vtblUIATextRangeArrayGetElement    = 4  // IUnknown(3) + 1
	vtblUIATextRangeExpandToEnclosing  = 6  // IUnknown(3) + 3
	vtblUIATextRangeGetBoundingRects   = 10 // IUnknown(3) + 7
	vtblUIATextRangeMove               = 13 // IUnknown(3) + 10

	uiaProbeBudget = 60 * time.Millisecond
	uiaCOMTimeout  = 50 // milliseconds
)

var (
	iidIAccessible               = windows.GUID{Data1: 0x618736e0, Data2: 0x3c3d, Data3: 0x11cf, Data4: [8]byte{0x81, 0x0c, 0x00, 0xaa, 0x00, 0x38, 0x9b, 0x71}}
	clsidCUIAutomation8          = windows.GUID{Data1: 0xe22ad333, Data2: 0xb25f, Data3: 0x460c, Data4: [8]byte{0x83, 0xd0, 0x05, 0x81, 0x10, 0x73, 0x95, 0xc9}}
	clsidCUIAutomation           = windows.GUID{Data1: 0xff48dba4, Data2: 0x60ef, Data3: 0x4201, Data4: [8]byte{0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e}}
	iidIUIAutomation             = windows.GUID{Data1: 0x30cbe57d, Data2: 0xd9d0, Data3: 0x452a, Data4: [8]byte{0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee}}
	iidIUIAutomation2            = windows.GUID{Data1: 0x34723aff, Data2: 0x0c9d, Data3: 0x49d0, Data4: [8]byte{0x98, 0x96, 0x7a, 0xb5, 0x2d, 0xf8, 0xcd, 0x8a}}
	iidIUIAutomationTextPattern  = windows.GUID{Data1: 0x32eba289, Data2: 0x3583, Data3: 0x42c9, Data4: [8]byte{0x9c, 0x59, 0x3b, 0x6d, 0x9a, 0x1e, 0x9b, 0x6a}}
	iidIUIAutomationTextPattern2 = windows.GUID{Data1: 0x506a921a, Data2: 0xfcc9, Data3: 0x409f, Data4: [8]byte{0xb2, 0x3b, 0x37, 0xeb, 0x74, 0x10, 0x68, 0x72}}
)

type winPoint struct {
	X, Y int32
}

type winRect struct {
	Left, Top, Right, Bottom int32
}

func (r winRect) toRect() Rect {
	return Rect{
		X:      int(r.Left),
		Y:      int(r.Top),
		Width:  int(r.Right - r.Left),
		Height: int(r.Bottom - r.Top),
	}
}

type winMonitorInfo struct {
	Size    uint32
	Monitor winRect
	Work    winRect
	Flags   uint32
}

type winGUIThreadInfo struct {
	Size        uint32
	Flags       uint32
	HwndActive  uintptr
	HwndFocus   uintptr
	HwndCapture uintptr
	HwndMenu    uintptr
	HwndMove    uintptr
	HwndCaret   uintptr
	RcCaret     winRect
}

// winVariant matches the 64-bit Windows VARIANT layout (24 bytes).
type winVariant struct {
	vt   uint16
	res1 uint16
	res2 uint16
	res3 uint16
	val1 uintptr
	val2 uintptr
}

var (
	enumMonitorsMu   sync.Mutex
	enumMonitorsOnce sync.Once
	enumMonitorsCB   uintptr
	enumMonitorsSink func(uintptr)
)

func enumeratePhysicalMonitors() []PhysicalMonitor {
	enumMonitorsMu.Lock()
	defer enumMonitorsMu.Unlock()
	enumMonitorsOnce.Do(func() {
		enumMonitorsCB = syscall.NewCallback(func(mon, _, _, _ uintptr) uintptr {
			if enumMonitorsSink != nil {
				enumMonitorsSink(mon)
			}
			return 1
		})
	})
	var out []PhysicalMonitor
	enumMonitorsSink = func(mon uintptr) {
		var mi winMonitorInfo
		mi.Size = uint32(unsafe.Sizeof(mi))
		if ok, _, _ := getMonitorInfoW.Call(mon, uintptr(unsafe.Pointer(&mi))); ok != 0 {
			out = append(out, PhysicalMonitor{
				ID:     int64(mon),
				Bounds: mi.Monitor.toRect(),
			})
		}
	}
	_, _, _ = enumDisplayMonitors.Call(0, 0, enumMonitorsCB, 0)
	enumMonitorsSink = nil
	return out
}

func isShellOrDesktopWindow(hwnd uintptr) bool {
	if hwnd == 0 {
		return true
	}
	var buf [64]uint16
	n, _, _ := getClassNameW.Call(hwnd, uintptr(unsafe.Pointer(&buf[0])), uintptr(len(buf)))
	if n == 0 {
		return false
	}
	switch windows.UTF16ToString(buf[:n]) {
	case "Progman", "WorkerW", "Shell_TrayWnd", "Shell_SecondaryTrayWnd":
		return true
	default:
		return false
	}
}

func monitorDPIForRect(r Rect) int {
	if shcore.Load() != nil || getDpiForMonitor.Find() != nil {
		return 96
	}
	wr := winRect{
		Left:   int32(r.X),
		Top:    int32(r.Y),
		Right:  int32(r.X + max(r.Width, 1)),
		Bottom: int32(r.Y + max(r.Height, 1)),
	}
	mon, _, _ := monitorFromRect.Call(uintptr(unsafe.Pointer(&wr)), monitorDefaultToNearest)
	if mon == 0 {
		return 96
	}
	var dx, dy uint32
	hr, _, _ := getDpiForMonitor.Call(mon, mdtEffectiveDPI, uintptr(unsafe.Pointer(&dx)), uintptr(unsafe.Pointer(&dy)))
	if int32(uint32(hr)) < 0 || dx == 0 {
		return 96
	}
	return int(dx)
}

// CapturePlacement captures physical monitor bounds, mouse position, target
// window bounds, and (in caret mode) the active text caret or compact input box
// while the external target still holds foreground focus.
func (p *windowsPlatform) CapturePlacement(mode string, target Target) (PlacementAnchor, bool) {
	anchor := PlacementAnchor{
		Monitors: enumeratePhysicalMonitors(),
	}
	var pt winPoint
	if ok, _, _ := getCursorPos.Call(uintptr(unsafe.Pointer(&pt))); ok != 0 {
		anchor.HasCursor = true
		anchor.Cursor = Point{X: int(pt.X), Y: int(pt.Y)}
	}

	hwnd := target.ID
	if hwnd != 0 && !isShellOrDesktopWindow(hwnd) {
		if valid, _, _ := isWindow.Call(hwnd); valid != 0 {
			if minimized, _, _ := isIconic.Call(hwnd); minimized == 0 {
				var wr winRect
				if ok, _, _ := getWindowRect.Call(hwnd, uintptr(unsafe.Pointer(&wr))); ok != 0 {
					r := wr.toRect()
					if r.Width > 0 && r.Height > 0 {
						anchor.HasWindow = true
						anchor.Window = r
					}
				}
			}
		}
	}

	if NormalizePosition(mode) == PositionCaret && hwnd != 0 && anchor.HasWindow {
		if fg, _, _ := getForeground.Call(); fg == hwnd {
			if kind, rect, ok := p.probeCaretOrInput(hwnd, anchor.Window); ok {
				anchor.Kind = kind
				anchor.Rect = rect
			}
		}
	}
	return anchor, len(anchor.Monitors) > 0
}

func (p *windowsPlatform) probeCaretOrInput(fgHwnd uintptr, windowRect Rect) (AnchorKind, Rect, bool) {
	// Layer 1: Win32 GetGUIThreadInfo (non-invasive, < 0.1ms).
	var gti winGUIThreadInfo
	gti.Size = uint32(unsafe.Sizeof(gti))
	tid, _, _ := getWindowPID.Call(fgHwnd, 0)
	var focusHwnd uintptr
	if tid != 0 {
		if ok, _, _ := getGUIThreadInfo.Call(tid, uintptr(unsafe.Pointer(&gti))); ok != 0 {
			focusHwnd = gti.HwndFocus
			caretHwnd := gti.HwndCaret
			if caretHwnd == 0 {
				caretHwnd = gti.HwndFocus
			}
			if caretHwnd != 0 && (gti.RcCaret.Right > gti.RcCaret.Left || gti.RcCaret.Bottom > gti.RcCaret.Top) {
				rc := gti.RcCaret
				_, _, _ = mapWindowPoints.Call(caretHwnd, 0, uintptr(unsafe.Pointer(&rc)), 2)
				r := rc.toRect()
				if caretRectPlausible(r, true, windowRect) {
					return AnchorCaret, r, true
				}
			}
		}
	}

	// Layer 2 & 3: MSAA + UI Automation on the bounded background worker so a
	// hung or slow target never blocks opening the Quick Panel past uiaProbeBudget.
	return p.probeAccessibilityCaret(fgHwnd, focusHwnd, windowRect)
}

func probeMSAACaret(focusHwnd, fgHwnd uintptr, windowRect Rect) (Rect, bool) {
	if oleacc.Load() != nil || accessibleObjectFromWnd.Find() != nil {
		return Rect{}, false
	}
	// Only call on 64-bit where VARIANT (24 bytes) is passed by reference in both
	// Windows x64 and Windows ARM64 calling conventions.
	if unsafe.Sizeof(uintptr(0)) != 8 {
		return Rect{}, false
	}
	candidates := [2]uintptr{focusHwnd, fgHwnd}
	for i, h := range candidates {
		if h == 0 || (i == 1 && h == candidates[0]) {
			continue
		}
		var acc uintptr
		hr, _, _ := accessibleObjectFromWnd.Call(
			h,
			objidCaret,
			uintptr(unsafe.Pointer(&iidIAccessible)),
			uintptr(unsafe.Pointer(&acc)),
		)
		if int32(uint32(hr)) < 0 || acc == 0 {
			continue
		}
		var left, top, width, height int32
		v := winVariant{vt: vtI4, val1: childIDShelf}
		locHR := comCall(
			acc,
			vtblIAccessibleAccLocation,
			uintptr(unsafe.Pointer(&left)),
			uintptr(unsafe.Pointer(&top)),
			uintptr(unsafe.Pointer(&width)),
			uintptr(unsafe.Pointer(&height)),
			uintptr(unsafe.Pointer(&v)),
		)
		comRelease(acc)
		if int32(uint32(locHR)) < 0 {
			continue
		}
		r := Rect{X: int(left), Y: int(top), Width: int(width), Height: int(height)}
		if caretRectPlausible(r, true, windowRect) {
			return r, true
		}
	}
	return Rect{}, false
}

type uiaProbeReq struct {
	fgHwnd     uintptr
	focusHwnd  uintptr
	windowRect Rect
	deadline   time.Time
	reply      chan uiaProbeResp
}

type uiaProbeResp struct {
	kind AnchorKind
	rect Rect
	ok   bool
}

type uiaWorker struct {
	req  chan uiaProbeReq
	stop chan struct{}
	done chan struct{}
}

func (p *windowsPlatform) ensureUIAWorker() *uiaWorker {
	p.uiaMu.Lock()
	defer p.uiaMu.Unlock()
	if p.closed {
		return nil
	}
	if p.uia == nil {
		w := &uiaWorker{
			req:  make(chan uiaProbeReq, 1),
			stop: make(chan struct{}),
			done: make(chan struct{}),
		}
		p.uia = w
		go w.run()
	}
	return p.uia
}

func (p *windowsPlatform) Close() {
	p.uiaMu.Lock()
	p.closed = true
	w := p.uia
	p.uia = nil
	p.uiaMu.Unlock()
	if w != nil {
		close(w.stop)
		select {
		case <-w.done:
		case <-time.After(100 * time.Millisecond):
		}
	}
}

func (p *windowsPlatform) probeAccessibilityCaret(fgHwnd, focusHwnd uintptr, windowRect Rect) (AnchorKind, Rect, bool) {
	if unsafe.Sizeof(uintptr(0)) != 8 {
		return AnchorNone, Rect{}, false
	}
	w := p.ensureUIAWorker()
	if w == nil {
		return AnchorNone, Rect{}, false
	}
	reply := make(chan uiaProbeResp, 1)
	req := uiaProbeReq{
		fgHwnd:     fgHwnd,
		focusHwnd:  focusHwnd,
		windowRect: windowRect,
		deadline:   time.Now().Add(uiaProbeBudget),
		reply:      reply,
	}
	select {
	case w.req <- req:
	default:
		// Worker is still finishing a timed-out COM call; fall back immediately.
		return AnchorNone, Rect{}, false
	}
	timer := time.NewTimer(uiaProbeBudget)
	defer timer.Stop()
	select {
	case resp := <-reply:
		return resp.kind, resp.rect, resp.ok
	case <-timer.C:
		return AnchorNone, Rect{}, false
	}
}

func (w *uiaWorker) run() {
	defer close(w.done)
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()

	comInit := false
	if ole32.Load() == nil && coInitializeEx.Find() == nil {
		hr, _, _ := coInitializeEx.Call(0, coinitMultithreaded)
		comInit = int32(uint32(hr)) >= 0
	}
	if comInit {
		defer coUninitialize.Call()
	}

	var uia uintptr
	if ole32.Load() == nil && coCreateInstance.Find() == nil {
		uia = createUIAutomation()
	}
	if uia != 0 {
		defer comRelease(uia)
	}

	for {
		select {
		case <-w.stop:
			return
		case req := <-w.req:
			if time.Now().After(req.deadline) {
				req.reply <- uiaProbeResp{}
				continue
			}
			// Layer 2: MSAA AccessibleObjectFromWindow(OBJID_CARET).
			if r, ok := probeMSAACaret(req.focusHwnd, req.fgHwnd, req.windowRect); ok {
				if fg, _, _ := getForeground.Call(); fg == req.fgHwnd {
					req.reply <- uiaProbeResp{kind: AnchorCaret, rect: r, ok: true}
					continue
				}
			}
			if uia == 0 || time.Now().After(req.deadline) {
				req.reply <- uiaProbeResp{}
				continue
			}
			// Layer 3: UI Automation (CUIAutomation8).
			kind, rect, ok := queryUIACaret(uia, req.windowRect)
			if ok {
				if fg, _, _ := getForeground.Call(); fg != req.fgHwnd {
					ok = false
					kind = AnchorNone
					rect = Rect{}
				}
			}
			req.reply <- uiaProbeResp{kind: kind, rect: rect, ok: ok}
		}
	}
}

func createUIAutomation() uintptr {
	var uia uintptr
	hr, _, _ := coCreateInstance.Call(
		uintptr(unsafe.Pointer(&clsidCUIAutomation8)),
		0,
		clsctxInprocServer,
		uintptr(unsafe.Pointer(&iidIUIAutomation)),
		uintptr(unsafe.Pointer(&uia)),
	)
	if int32(uint32(hr)) < 0 || uia == 0 {
		hr, _, _ = coCreateInstance.Call(
			uintptr(unsafe.Pointer(&clsidCUIAutomation)),
			0,
			clsctxInprocServer,
			uintptr(unsafe.Pointer(&iidIUIAutomation)),
			uintptr(unsafe.Pointer(&uia)),
		)
		if int32(uint32(hr)) < 0 || uia == 0 {
			return 0
		}
	}
	if uia2 := comQueryInterface(uia, &iidIUIAutomation2); uia2 != 0 {
		_ = comCall(uia2, vtblUIA2PutConnectionTimeout, uintptr(uiaCOMTimeout))
		_ = comCall(uia2, vtblUIA2PutTransactionTimeout, uintptr(uiaCOMTimeout))
		comRelease(uia2)
	}
	return uia
}

// queryUIACaret reads ONLY geometric bounding rectangles from the focused UIA
// element; it never queries text content, control names, or values.
func queryUIACaret(uia uintptr, windowRect Rect) (AnchorKind, Rect, bool) {
	var elem uintptr
	hr := comCall(uia, vtblUIAGetFocusedElement, uintptr(unsafe.Pointer(&elem)))
	if int32(uint32(hr)) < 0 || elem == 0 {
		return AnchorNone, Rect{}, false
	}
	defer comRelease(elem)

	// 3a: IUIAutomationTextPattern2::GetCaretRange (Chrome, Edge, VS Code, Electron, WinUI 3, WPF).
	var tp2 uintptr
	hr = comCall(
		elem,
		vtblUIAElemGetCurrentPatternAs,
		uintptr(uiaTextPattern2ID),
		uintptr(unsafe.Pointer(&iidIUIAutomationTextPattern2)),
		uintptr(unsafe.Pointer(&tp2)),
	)
	if int32(uint32(hr)) >= 0 && tp2 != 0 {
		var isActive int32
		var textRange uintptr
		caretHR := comCall(
			tp2,
			vtblUIATextPattern2GetCaretRange,
			uintptr(unsafe.Pointer(&isActive)),
			uintptr(unsafe.Pointer(&textRange)),
		)
		comRelease(tp2)
		if int32(uint32(caretHR)) >= 0 && textRange != 0 {
			r, ok := extractTextRangeRect(textRange, windowRect)
			comRelease(textRange)
			if ok && isActive != 0 {
				return AnchorCaret, r, true
			}
		}
	}

	// 3b: IUIAutomationTextPattern::GetSelection (Windows Terminal and older TextPattern controls).
	var tp uintptr
	hr = comCall(
		elem,
		vtblUIAElemGetCurrentPatternAs,
		uintptr(uiaTextPatternID),
		uintptr(unsafe.Pointer(&iidIUIAutomationTextPattern)),
		uintptr(unsafe.Pointer(&tp)),
	)
	if int32(uint32(hr)) >= 0 && tp != 0 {
		var ranges uintptr
		selHR := comCall(tp, vtblUIATextPatternGetSelection, uintptr(unsafe.Pointer(&ranges)))
		comRelease(tp)
		if int32(uint32(selHR)) >= 0 && ranges != 0 {
			var count int32
			lenHR := comCall(ranges, vtblUIATextRangeArrayGetLength, uintptr(unsafe.Pointer(&count)))
			if int32(uint32(lenHR)) >= 0 && count > 0 {
				var textRange uintptr
				elemHR := comCall(ranges, vtblUIATextRangeArrayGetElement, 0, uintptr(unsafe.Pointer(&textRange)))
				if int32(uint32(elemHR)) >= 0 && textRange != 0 {
					r, ok := extractTextRangeRect(textRange, windowRect)
					comRelease(textRange)
					if ok {
						comRelease(ranges)
						return AnchorCaret, r, true
					}
				}
			}
			comRelease(ranges)
		}
	}

	// 3c: Compact single-line input control fallback (e.g. search bar or address bar).
	var controlType int32
	if int32(uint32(comCall(elem, vtblUIAElemGetCurrentControlType, uintptr(unsafe.Pointer(&controlType))))) >= 0 {
		if controlType == uiaEditControlTypeID || controlType == uiaComboBoxControlTypeID {
			var br winRect
			if int32(uint32(comCall(elem, vtblUIAElemGetCurrentBoundingRect, uintptr(unsafe.Pointer(&br))))) >= 0 {
				r := br.toRect()
				dpi := monitorDPIForRect(r)
				if inputControlRectPlausible(r, dpi, true, windowRect) {
					return AnchorInputControl, r, true
				}
			}
		}
	}

	return AnchorNone, Rect{}, false
}

func extractTextRangeRect(textRange uintptr, windowRect Rect) (Rect, bool) {
	if r, ok := readTextRangeBoundingRect(textRange); ok && caretRectPlausible(r, true, windowRect) {
		return r, true
	}

	// Degenerate 0-width ranges often return an empty bounding array; expand to
	// the enclosing character to obtain line geometry.
	if int32(uint32(comCall(textRange, vtblUIATextRangeExpandToEnclosing, uintptr(uiaTextUnitCharacter)))) >= 0 {
		if r, ok := readTextRangeBoundingRect(textRange); ok {
			caret := Rect{X: r.X, Y: r.Y, Width: 2, Height: r.Height}
			if caretRectPlausible(caret, true, windowRect) {
				return caret, true
			}
		}
	}

	// At end-of-line or end-of-document, step back one character and anchor to its trailing edge.
	var moved int32
	if int32(uint32(comCall(textRange, vtblUIATextRangeMove, uintptr(uiaTextUnitCharacter), uintptr(uint32(0xFFFFFFFF)), uintptr(unsafe.Pointer(&moved))))) >= 0 && moved != 0 {
		if int32(uint32(comCall(textRange, vtblUIATextRangeExpandToEnclosing, uintptr(uiaTextUnitCharacter)))) >= 0 {
			if r, ok := readTextRangeBoundingRect(textRange); ok {
				caret := Rect{X: r.X + r.Width, Y: r.Y, Width: 2, Height: r.Height}
				if caretRectPlausible(caret, true, windowRect) {
					return caret, true
				}
			}
		}
	}

	return Rect{}, false
}

func readTextRangeBoundingRect(textRange uintptr) (Rect, bool) {
	if oleaut32.Load() != nil {
		return Rect{}, false
	}
	var psa uintptr
	hr := comCall(textRange, vtblUIATextRangeGetBoundingRects, uintptr(unsafe.Pointer(&psa)))
	if int32(uint32(hr)) < 0 || psa == 0 {
		return Rect{}, false
	}
	defer safeArrayDestroy.Call(psa)

	var lbound, ubound int32
	hrL, _, _ := safeArrayGetLBound.Call(psa, 1, uintptr(unsafe.Pointer(&lbound)))
	hrU, _, _ := safeArrayGetUBound.Call(psa, 1, uintptr(unsafe.Pointer(&ubound)))
	if int32(uint32(hrL)) < 0 || int32(uint32(hrU)) < 0 {
		return Rect{}, false
	}
	count := int(ubound - lbound + 1)
	if count < 4 {
		return Rect{}, false
	}
	var data uintptr
	hrA, _, _ := safeArrayAccessData.Call(psa, uintptr(unsafe.Pointer(&data)))
	if int32(uint32(hrA)) < 0 || data == 0 {
		return Rect{}, false
	}
	defer safeArrayUnaccessData.Call(psa)

	vals := unsafe.Slice((*float64)(nativePtr(data)), count)
	left, top, width, height := vals[0], vals[1], vals[2], vals[3]
	if math.IsNaN(left) || math.IsNaN(top) || math.IsNaN(width) || math.IsNaN(height) ||
		math.IsInf(left, 0) || math.IsInf(top, 0) || math.IsInf(width, 0) || math.IsInf(height, 0) {
		return Rect{}, false
	}
	return Rect{
		X:      int(math.Round(left)),
		Y:      int(math.Round(top)),
		Width:  int(math.Round(width)),
		Height: int(math.Round(height)),
	}, true
}

// nativePtr turns an OS/COM pointer returned as uintptr into an unsafe.Pointer
// without triggering checkptr/go-vet false positives.
func nativePtr(p uintptr) unsafe.Pointer {
	return *(*unsafe.Pointer)(unsafe.Pointer(&p))
}

// comCall invokes COM vtable method index on obj.
//
//go:uintptrescapes
func comCall(obj uintptr, index int, args ...uintptr) uintptr {
	vtbl := *(*uintptr)(nativePtr(obj))
	fn := *(*uintptr)(unsafe.Add(nativePtr(vtbl), index*int(unsafe.Sizeof(uintptr(0)))))
	var a [10]uintptr
	a[0] = obj
	n := copy(a[1:], args)
	r, _, _ := syscall.SyscallN(fn, a[:n+1]...)
	return r
}

func comRelease(obj uintptr) {
	if obj != 0 {
		_ = comCall(obj, vtblRelease)
	}
}

func comQueryInterface(obj uintptr, iid *windows.GUID) uintptr {
	if obj == 0 {
		return 0
	}
	var out uintptr
	hr := comCall(obj, vtblQueryInterface, uintptr(unsafe.Pointer(iid)), uintptr(unsafe.Pointer(&out)))
	if int32(uint32(hr)) < 0 {
		return 0
	}
	return out
}
