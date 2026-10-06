package main

import (
	"context"
	"log"
	"strconv"
	"time"

	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

// backgroundNotice is the bilingual "still running" reassurance shown when the GUI hands off to the
// background daemon (issue #1129: without it the GUI process exiting looks like a crash).
const backgroundNotice = "UniClipboard 仍在后台运行，点应用图标可重新打开窗口。\n" +
	"Still running in the background — open it from the app icon to show the window again."

// notificationAction is delivered to the frontend when the user clicks a notification, in the shape the
// Tauri plugin's `onAction` handler receives.
const notificationActionEvent = "notification://action"

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
		payload := map[string]any{}
		if id, err := strconv.Atoi(result.Response.ID); err == nil {
			payload["id"] = id
		}
		h.emit(notificationActionEvent, payload)
	})
}

func init() {
	register(map[string]commandFunc{
		"host_notification_permission": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			return h.notificationsGranted(), nil
		},
		"host_notification_request_permission": func(_ context.Context, h *HostService, _ commandArgs) (any, error) {
			if h.requestNotificationPermission() {
				return "granted", nil
			}
			return "denied", nil
		},
		"host_notification_send": func(_ context.Context, h *HostService, args commandArgs) (any, error) {
			var options struct {
				ID    *int   `json:"id"`
				Title string `json:"title"`
				Body  string `json:"body"`
			}
			if err := args.decode("options", &options); err != nil {
				return nil, err
			}
			id := "n-" + strconv.FormatInt(time.Now().UnixNano(), 10)
			if options.ID != nil {
				id = strconv.Itoa(*options.ID) // a stable id replaces an earlier notification of the same kind
			}
			return nil, wrapInternal(h.notify(id, options.Title, options.Body))
		},
	})
}
