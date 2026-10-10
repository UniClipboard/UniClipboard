package main

import (
	"log"
	"strconv"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

// backgroundNotice is the bilingual "still running" reassurance shown when the GUI hands off to the
// background daemon (issue #1129: without it the GUI process exiting looks like a crash).
const backgroundNotice = "UniClipboard 仍在后台运行，点应用图标可重新打开窗口。\n" +
	"Still running in the background — open it from the app icon to show the window again."

func (h *HostService) notificationsGranted() bool {
	if granted, handled := notifyPermissionOverride(); handled {
		return granted
	}
	granted, err := h.notifier.CheckNotificationAuthorization()
	return err == nil && granted
}

func (h *HostService) requestNotificationPermission() bool {
	if granted, handled := notifyPermissionOverride(); handled {
		return granted
	}
	granted, err := h.notifier.RequestNotificationAuthorization()
	return err == nil && granted
}

// notify shows a system notification. It is best effort: a failure is logged and never blocks the caller.
func (h *HostService) notify(id, title, body string) error {
	if handled, err := notifyOverride(id, title, body); handled {
		return err
	}
	return h.notifier.SendNotification(notifications.NotificationOptions{ID: id, Title: title, Body: body})
}

// enterLightweightMode leaves only the daemon running: tell the user, then exit the GUI keeping the daemon.
func (h *HostService) enterLightweightMode() {
	if err := h.notify("lightweight-"+strconv.FormatInt(time.Now().UnixNano(), 10), "UniClipboard", backgroundNotice); err != nil {
		log.Printf("failed to show the lightweight-mode notification: %v", err)
	} else {
		time.Sleep(300 * time.Millisecond) // let the notification post before the process exits
	}
	h.quit(true)
}

func (h *HostService) watchNotificationClicks() {
	h.notifier.OnNotificationResponse(func(result notifications.NotificationResult) {
		e2eNotificationResponse(result.Response.ID)
		var payload NotificationAction
		if id, err := strconv.Atoi(result.Response.ID); err == nil {
			payload.ID = &id
		}
		h.emit(notificationActionEvent, payload)
	})
}

// HostNotificationPermission reports whether system notifications are allowed. It is a command behind the
// page's notification module (frontend/src/host/notification.ts).
//
//uc:errors none
//uc:os all=real
//uc:adapter @/host/notification
func (h *HostService) HostNotificationPermission() bool {
	return h.notificationsGranted()
}

// HostNotificationRequestPermission asks the system for notification permission (adapter command).
//
//uc:errors none
//uc:os all=real
//uc:adapter @/host/notification
func (h *HostService) HostNotificationRequestPermission() NotificationPermission {
	if h.requestNotificationPermission() {
		return NotificationGranted
	}
	return NotificationDenied
}

// HostNotificationSend shows a system notification (adapter command). A stable ID replaces an earlier
// notification of the same kind.
//
//uc:errors command InternalError
//uc:os all=real
//uc:adapter @/host/notification
func (h *HostService) HostNotificationSend(options HostNotification) error {
	id := "n-" + strconv.FormatInt(time.Now().UnixNano(), 10)
	if options.ID != nil {
		id = strconv.Itoa(*options.ID)
	}
	return hostapi.Internal(h.notify(id, options.Title, options.Body))
}
