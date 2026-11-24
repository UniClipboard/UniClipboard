package nativeboard

import (
	"errors"
	"mime"
	"net/url"
	"os"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"syscall"
	"unsafe"

	"github.com/ebitengine/purego/objc"

	"github.com/uniclipboard/unic/internal/clip"
)

const maxRepresentation = 64 << 20

// Board accesses the system pasteboard without starting a graphical application.
type Board struct{ name string }

// ActiveConsoleUser reports whether the process owns the active macOS console.
func ActiveConsoleUser() bool {
	console, err := os.Stat("/dev/console")
	if err != nil {
		return false
	}
	owner, ok := console.Sys().(*syscall.Stat_t)
	return ok && owner.Uid != 0 && owner.Uid == uint32(os.Geteuid())
}

func New() (*Board, error) {
	if !ActiveConsoleUser() {
		return nil, errors.New("macOS desktop session is unavailable")
	}
	if !appKit() {
		return nil, errors.New("macOS pasteboard is unavailable")
	}
	return &Board{}, nil
}

func changeCount(board objc.ID) uint64 {
	return uint64(objc.Send[int64](board, objc.RegisterName("changeCount")))
}

func (b *Board) use(work func(objc.ID) error) error {
	if !ActiveConsoleUser() {
		return errors.New("macOS desktop session is unavailable")
	}
	runtime.LockOSThread()
	defer runtime.UnlockOSThread()
	pool := send(send(objc.ID(objc.GetClass("NSAutoreleasePool")), "alloc"), "init")
	defer send(pool, "drain")
	var board objc.ID
	if b.name == "" {
		board = pasteboard()
	} else {
		board = send(objc.ID(objc.GetClass("NSPasteboard")), "pasteboardWithName:", nsString(b.name))
	}
	if board == 0 {
		return errors.New("macOS pasteboard is unavailable")
	}
	return work(board)
}

var contentTypes = []struct {
	format string
	types  []string
}{
	{clip.Text, []string{"public.utf8-plain-text", "public.utf16-plain-text", "public.utf16-external-plain-text", "NSStringPboardType"}},
	{clip.HTML, []string{"public.html", "NSHTMLPboardType"}},
	{clip.PNG, []string{"public.png", "public.tiff", "NSTIFFPboardType"}},
}

func nsBytes(data objc.ID) ([]byte, error) {
	if data == 0 {
		return nil, nil
	}
	n := objc.Send[uint64](data, objc.RegisterName("length"))
	if n > maxRepresentation {
		return nil, errors.New("pasteboard representation exceeds the size limit")
	}
	p := objc.Send[*byte](data, objc.RegisterName("bytes"))
	return append([]byte(nil), unsafe.Slice(p, int(n))...), nil
}

func resolveFileURL(raw string) string {
	raw = strings.TrimRight(raw, "\x00")
	if raw == "" {
		return ""
	}
	u, err := url.Parse(raw)
	if err != nil || !strings.EqualFold(u.Scheme, "file") || (u.Host != "" && !strings.EqualFold(u.Host, "localhost")) {
		return ""
	}
	if strings.HasPrefix(u.Path, "/.file/") {
		// Finder writes file reference URLs (file:///.file/id=...) to public.file-url;
		// resolve them through NSURL.filePathURL instead of treating /.file/... as a POSIX path.
		nsu := send(objc.ID(objc.GetClass("NSURL")), "URLWithString:", nsString(raw))
		if nsu == 0 {
			return ""
		}
		resolved := send(nsu, "filePathURL")
		if resolved == 0 {
			return ""
		}
		path := goString(send(resolved, "path"))
		if filepath.IsAbs(path) {
			return path
		}
		return ""
	}
	if filepath.IsAbs(u.Path) {
		return u.Path
	}
	return ""
}

func filePaths(board objc.ID) []string {
	items := send(board, "pasteboardItems")
	var paths []string
	for i, count := uint64(0), objc.Send[uint64](items, objc.RegisterName("count")); i < count; i++ {
		item := send(items, "objectAtIndex:", i)
		value := goString(send(item, "stringForType:", nsString("public.file-url")))
		if path := resolveFileURL(value); path != "" {
			paths = append(paths, path)
		}
	}
	if len(paths) > 0 {
		return paths
	}
	legacy := send(board, "propertyListForType:", nsString("NSFilenamesPboardType"))
	for i, count := uint64(0), objc.Send[uint64](legacy, objc.RegisterName("count")); i < count; i++ {
		path := goString(send(legacy, "objectAtIndex:", i))
		if filepath.IsAbs(path) {
			paths = append(paths, path)
		}
	}
	return paths
}

func (b *Board) Read() (snapshot clip.Snapshot, err error) {
	err = b.use(func(board objc.ID) error {
		token := changeCount(board)
		snapshot.CopyID = strconv.FormatUint(token, 10)
		for _, path := range filePaths(board) {
			path = filepath.Clean(path)
			file := clip.File{Name: filepath.Base(path), Path: path}
			if info, e := os.Stat(path); e == nil {
				file.Size, file.Directory = info.Size(), info.IsDir()
			}
			snapshot.Files = append(snapshot.Files, file)
		}
		if len(snapshot.Files) == 0 {
			for _, format := range contentTypes {
				for _, name := range format.types {
					typ := nsString(name)
					data := send(board, "dataForType:", typ)
					if data == 0 {
						continue
					}
					if format.format == clip.Text && name != format.types[0] {
						value := send(board, "stringForType:", typ)
						data = send(value, "dataUsingEncoding:", uint64(4))
					}
					if format.format == clip.PNG && name != format.types[0] {
						rep := send(objc.ID(objc.GetClass("NSBitmapImageRep")), "imageRepWithData:", data)
						data = send(rep, "representationUsingType:properties:", uint64(4), send(objc.ID(objc.GetClass("NSDictionary")), "dictionary"))
					}
					bytes, e := nsBytes(data)
					if e != nil {
						return e
					}
					if len(bytes) > 0 {
						snapshot.Parts = append(snapshot.Parts, clip.Part{Format: format.format, Data: bytes})
						break
					}
				}
			}
		}
		if b.name == "" && !snapshot.Empty() {
			_, snapshot.SourceApplication = pasteboardSource(board)
		}
		if changeCount(board) != token {
			return errors.New("clipboard changed during read")
		}
		return nil
	})
	return
}

// ReadFileImage reads an image alternate for the unchanged single file copy.
func (b *Board) ReadFileImage(copyID string) (png []byte) {
	c := objc.RegisterName("count")
	length := objc.RegisterName("length")
	_ = b.use(func(board objc.ID) error {
		token := changeCount(board)
		if strconv.FormatUint(token, 10) != copyID {
			return nil
		}
		paths := filePaths(board)
		items := send(board, "pasteboardItems")
		if len(paths) != 1 || objc.Send[uint64](items, c) != 1 ||
			!strings.HasPrefix(mime.TypeByExtension(strings.ToLower(filepath.Ext(paths[0]))), "image/") {
			return nil
		}
		item := send(items, "objectAtIndex:", uint64(0))
		for _, name := range []string{"public.png", "public.tiff", "NSTIFFPboardType"} {
			data := send(item, "dataForType:", nsString(name))
			if data == 0 || objc.Send[uint64](data, length) > maxRepresentation {
				continue
			}
			if name != "public.png" {
				rep := send(objc.ID(objc.GetClass("NSBitmapImageRep")), "imageRepWithData:", data)
				data = send(rep, "representationUsingType:properties:", uint64(4), send(objc.ID(objc.GetClass("NSDictionary")), "dictionary"))
			}
			if data == 0 || objc.Send[uint64](data, length) > clip.MaxFileImageBytes {
				continue
			}
			value, err := nsBytes(data)
			if err == nil && len(value) > 0 && changeCount(board) == token {
				png = value
				break
			}
		}
		return nil
	})
	return png
}

func (b *Board) Write(snapshot clip.Snapshot) error {
	return b.use(func(board objc.ID) error {
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		if len(snapshot.Files) > 0 {
			for _, file := range snapshot.Files {
				item := send(objc.ID(objc.GetClass("NSURL")), "fileURLWithPath:", nsString(file.Path))
				send(items, "addObject:", item)
			}
		} else if len(snapshot.Parts) > 0 {
			item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
			defer send(item, "release")
			for _, part := range snapshot.Parts {
				var typ string
				for _, format := range contentTypes {
					if format.format == part.Format {
						typ = format.types[0]
						break
					}
				}
				if typ == "" || len(part.Data) > maxRepresentation {
					return errors.New("unsupported pasteboard representation")
				}
				data := send(objc.ID(objc.GetClass("NSData")), "dataWithBytes:length:", unsafe.Pointer(unsafe.SliceData(part.Data)), uint64(len(part.Data)))
				runtime.KeepAlive(part.Data)
				if !objc.Send[bool](item, objc.RegisterName("setData:forType:"), data, nsString(typ)) {
					return errors.New("could not prepare pasteboard representation")
				}
			}
			send(items, "addObject:", item)
		}
		send(board, "clearContents")
		if !snapshot.Empty() && !objc.Send[bool](board, objc.RegisterName("writeObjects:"), items) {
			return errors.New("could not write pasteboard")
		}
		return nil
	})
}
