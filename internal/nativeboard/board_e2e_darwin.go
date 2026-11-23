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
