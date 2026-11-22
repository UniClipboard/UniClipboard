// Package preview defines data supplied by isolated desktop previews.
// It contains no fixtures, rendering, service access or lifecycle behavior.
package preview

import (
	"github.com/egoist/mygo/ui"

	"github.com/uniclipboard/unic/app/desktop/internal/desktopkit"
)

type Entry struct {
	ID, Title, Text, Source, Date, Size string
	SourceAppName, SourceAppID          string
	SourceAppIcon                       ui.ImageSource
	Kind                                desktopkit.ContentKind
	Tags                                []string
	Pinned                              bool
}
type SyncChannel struct {
	ID, ImplementationID, TransportKind, State, Policy string
	Pairing, ExternalChannels, Restart                 bool
}
type DeviceBinding struct {
	ID, InstanceID, ImplementationID, State string
	SyncEnabled                             bool
	Connection, LastActive, LastError       string
}
type Device struct {
	ID, Name, UserName, ReportedName, DeviceID string
	Icon                                       desktopkit.IconName
	Online                                     bool
	LastSeen, Fingerprint                      string
	Bindings                                   []DeviceBinding
}
type ExternalChannel struct {
	ID, Name, BackendID, LastSeen, LastError string
	Enabled, AutoApply, RemoteRevocation     bool
}
type PendingPath struct {
	ID, InstanceID, State string
}
type PendingDevice struct {
	ID, Name, Via, Fingerprint string
	ByCode                     bool
	Icon                       desktopkit.IconName
	Paths                      []PendingPath
}
type DeviceCandidate struct {
	ID, Name, Introducer, BackendID string
	Icon                            desktopkit.IconName
}
type Tag struct {
	ID, Name string
	Color    desktopkit.TagColor
	Sidebar  bool
}
type Desktop struct {
	Page, Theme, Layout, Selected, DeviceName string
	InviteCode, InviteMode, Reference         string
	Options                                   map[string]bool
	Choices                                   map[string]string
	Entries                                   []Entry
	SyncChannels                              []SyncChannel
	Devices                                   []Device
	Channels                                  []ExternalChannel
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
