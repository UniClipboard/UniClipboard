package clipboardintegration

import (
	"fmt"

	"github.com/uniclipboard/unic/internal/clip"
	"github.com/uniclipboard/unic/internal/localcontent"
)

var contentInput = localcontent.Input

// describe says what kind of thing a snapshot is, for the event log. It never
// includes the content or file names.
func describe(s clip.Snapshot) string {
	switch {
	case len(s.Files) == 1:
		return "1 file"
	case len(s.Files) > 1:
		return fmt.Sprintf("%d files", len(s.Files))
	}
	for _, p := range s.Parts {
		if p.Format == clip.PNG {
			return "image"
		}
	}
	return "text"
}
