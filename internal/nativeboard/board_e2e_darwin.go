//go:build unic_e2e

package nativeboard

import (
	"errors"
	"strings"

	"github.com/ebitengine/purego/objc"
)

// NamedForE2E targets a private pasteboard in an isolated process test.
func NamedForE2E(name string) (*Board, error) {
	if !strings.HasPrefix(name, "org.uniclipboard.e2e.") {
		return nil, errors.New("invalid test pasteboard name")
	}
	b, err := New()
	if err == nil {
		b.name = name
	}
	return b, err
}

// ReleaseForE2E removes the named pasteboard after all fixture processes exit.
func (b *Board) ReleaseForE2E() error {
	if b.name == "" {
		return errors.New("refusing to release the system pasteboard")
	}
	return b.use(func(board objc.ID) error {
		send(board, "releaseGlobally")
		return nil
	})
}

// WriteFileReferencesForE2E writes Finder-style file reference URLs to a private test pasteboard.
func (b *Board) WriteFileReferencesForE2E(paths ...string) error {
	if b.name == "" {
		return errors.New("refusing to write to the system pasteboard")
	}
	return b.use(func(board objc.ID) error {
		items := send(objc.ID(objc.GetClass("NSMutableArray")), "array")
		for _, path := range paths {
			u := send(objc.ID(objc.GetClass("NSURL")), "fileURLWithPath:", nsString(path))
			ref := send(u, "fileReferenceURL")
			if ref == 0 {
				return errors.New("could not build file reference URL")
			}
			item := send(send(objc.ID(objc.GetClass("NSPasteboardItem")), "alloc"), "init")
			defer send(item, "release")
			send(item, "setString:forType:", send(ref, "absoluteString"), nsString("public.file-url"))
			send(items, "addObject:", item)
		}
		send(board, "clearContents")
		if !objc.Send[bool](board, objc.RegisterName("writeObjects:"), items) {
			return errors.New("could not write file reference pasteboard")
		}
		return nil
	})
}
