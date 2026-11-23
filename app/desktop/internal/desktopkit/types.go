package desktopkit

type IconName string

const (
	IconAdd          IconName = "add"
	IconBack         IconName = "back"
	IconChat         IconName = "chat"
	IconChevronDown  IconName = "chevronDown"
	IconChevronRight IconName = "chevronRight"
	IconCopy         IconName = "copy"
	IconMaximize     IconName = "maximize"
	IconDelete       IconName = "delete"
	IconDesktop      IconName = "desktop"
	IconDevices      IconName = "devices"
	IconDismiss      IconName = "dismiss"
	IconDownload     IconName = "download"
	IconEdit         IconName = "edit"
	IconFile         IconName = "file"
	IconFolder       IconName = "folder"
	IconText         IconName = "text"
	IconCode         IconName = "code"
	IconLink         IconName = "link"
	IconImage        IconName = "image"
	IconFilter       IconName = "filter"
	IconGlobe        IconName = "globe"
	IconGrid         IconName = "grid"
	IconHistory      IconName = "history"
	IconInfo         IconName = "info"
	IconKeyboard     IconName = "keyboard"
	IconList         IconName = "list"
	IconLock         IconName = "lock"
	IconMerge        IconName = "merge"
	IconMoon         IconName = "moon"
	IconMore         IconName = "more"
	IconPackage      IconName = "package"
	IconPhone        IconName = "phone"
	IconPin          IconName = "pin"
	IconQuickPanel   IconName = "quickPanel"
	IconRemove       IconName = "remove"
	IconRetry        IconName = "retry"
	IconSearch       IconName = "search"
	IconSend         IconName = "send"
	IconSettings     IconName = "settings"
	IconShield       IconName = "shield"
	IconSidebar      IconName = "sidebar"
	IconSun          IconName = "sun"
	IconTag          IconName = "tag"
	IconUnlock       IconName = "unlock"
	IconUpdate       IconName = "update"
)

// ControlSize is a semantic size step; XLarge is the prominent primary action of an action
type ControlSize int

const (
	SizeMedium ControlSize = iota // zero value = the default
	SizeSmall
	SizeLarge
	SizeXLarge
)

// ButtonVariant: Primary = the single main action of a view; Standard = ordinary push
// button; Subtle = no fill at rest (command bars, icon buttons); Destructive removes data;
// Link is an inline text command.
type ButtonVariant int

const (
	Standard ButtonVariant = iota // zero value = the default
	Primary
	Subtle
	Destructive
	Link
)

// TagColor is a palette name ("orange","blue","green","purple","gray") or a custom "#rrggbb";
// "" draws neutral.
type TagColor string

// NoticeTone colours an inline notice.
type NoticeTone int

const (
	ToneInfo NoticeTone = iota
	ToneSuccess
	ToneWarning
	ToneError
)

// StatusTone colours a status mark.
type StatusTone int

const (
	StatusNeutral StatusTone = iota
	StatusPositive
	StatusInfo
	StatusWarning
	StatusCritical
)

// TextRole / TextTone are the type ramp and semantic text colour.
type TextRole int

const (
	RoleBody TextRole = iota
	RoleTitle
	RoleHeadline
	RoleCallout
	RoleCaption
)

type TextTone int

const (
	TonePrimary TextTone = iota
	ToneSecondary
	ToneTertiary
	TonePositive
	ToneCritical
)

// ContentKind is the type of a clipboard entry.
type ContentKind int

const (
	KindText ContentKind = iota
	KindCode
	KindLink
	KindImage
	KindFile
	KindFolder
)

// SurfaceRegion: Sidebar shows the platform material where available, Content is opaque.
type SurfaceRegion int

const (
	RegionContent SurfaceRegion = iota
	RegionSidebar
)
