// Package preview defines data supplied by isolated desktop previews.
// It contains no fixtures, rendering, service access or lifecycle behavior.
package preview

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/uikit"
)

type Entry struct {
	ID, Title, Text, Source, Date, Size string
	Kind                                uikit.ContentKind
	Tags                                []string
	Pinned                              bool
}
type DevicePath struct {
	Name, State, LastSync string
	Failed                bool
}
type Device struct {
	ID, Name, Platform                     string
	Icon                                   uikit.IconName
	Online                                 bool
	Latency                                int
	Link, LastSeen, Fingerprint, Direction string
	Text, Image, File                      bool
	Paths                                  []DevicePath
}
type PendingDevice struct {
	ID, Name, Platform, Via, Fingerprint string
	Icon                                 uikit.IconName
}
type DeviceCandidate struct {
	ID, Name, Introducer, BackendID string
	Icon                            uikit.IconName
}
type Tag struct {
	ID, Name string
	Color    uikit.TagColor
	Sidebar  bool
}
type Desktop struct {
	Page, Theme, Layout, Selected, DeviceName string
	InviteCode, InviteMode, Reference         string
	Options                                   map[string]bool
	Choices                                   map[string]string
	Entries                                   []Entry
	Devices                                   []Device
	Pending                                   []PendingDevice
	Candidates                                []DeviceCandidate
	Tags                                      []Tag
	Image                                     ui.ImageSource
	Quick                                     Quick
}
type QuickEntry struct {
	ID, Title, Text, Kind, Source, Age string
	Tags                               []string
	Favorite, Lost                     bool
}
type Quick struct {
	Theme, Mode string
	Entries     []QuickEntry
	Image       ui.ImageSource
}
