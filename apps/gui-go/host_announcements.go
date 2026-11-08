package main

import (
	"context"
	"net/http"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/announcements"
	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/buildinfo"
)

// GetReleaseAnnouncements reads public notices for the installed release, independently of update availability.
//
//uc:errors text
//uc:os all=real
func (h *HostService) GetReleaseAnnouncements(ctx context.Context, locale string) ([]announcements.Announcement, error) {
	if uiChild {
		var result []announcements.Announcement
		err := h.ownerCall(ctx, "release-announcements", locale, &result)
		return result, err
	}
	ctx, cancel := commandContext(ctx, "get_release_announcements")
	defer cancel()
	return announcements.Fetch(ctx, &http.Client{Timeout: 8 * time.Second}, announcements.Endpoint,
		buildinfo.PackageVersion, string(update.DetectChannel(buildinfo.PackageVersion)), locale)
}
