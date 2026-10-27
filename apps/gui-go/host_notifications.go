package main

import (
	"context"
	"log"
	"net/http"

	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
	"strconv"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/hostapi"
	"github.com/wailsapp/wails/v3/pkg/services/notifications"
)

// backgroundNotices use the same resolved locale as the tray menu.
var backgroundNotices = map[string]string{
	"zh-CN": "UniClipboard 仍在后台运行，点应用图标可重新打开窗口。",
	"zh-TW": "UniClipboard 仍在背景執行，點選應用程式圖示即可重新開啟視窗。",
	"ja-JP": "UniClipboard はバックグラウンドで動作しています。アプリのアイコンをクリックすると、ウィンドウを再び開けます。",
	"ru-RU": "UniClipboard продолжает работать в фоновом режиме. Нажмите на значок приложения, чтобы снова открыть окно.",
	"pt-BR": "O UniClipboard continua em execução em segundo plano. Clique no ícone do aplicativo para abrir a janela novamente.",
	"en":    "Still running in the background — open it from the app icon to show the window again.",
}

func (h *HostService) notificationsGranted() bool {
	if uiChild {
		var result bool
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		if err := h.ownerCall(ctx, "notification-permission", false, &result); err != nil {
			log.Printf("notification permission request failed: %v", err)
		}
		return result
	}
	if granted, handled := notifyPermissionOverride(); handled {
		return granted
	}
	granted, err := h.notifier.CheckNotificationAuthorization()
	return err == nil && granted
}

func (h *HostService) requestNotificationPermission() bool {
	if uiChild {
		var result bool
		ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
		defer cancel()
		if err := h.ownerCall(ctx, "notification-permission", true, &result); err != nil {
			log.Printf("notification authorization failed: %v", err)
		}
		return result
	}
	if granted, handled := notifyPermissionOverride(); handled {
		return granted
	}
	granted, err := h.notifier.RequestNotificationAuthorization()
	return err == nil && granted
}

// notify shows a system notification. It is best effort: a failure is logged and never blocks the caller.
func (h *HostService) notify(id, title, body string) error {
	if uiChild {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		return h.ownerCall(ctx, "notification-send", struct{ ID, Title, Body string }{id, title, body}, nil)
	}
	if handled, err := notifyOverride(id, title, body); handled {
		return err
	}
	return h.notifier.SendNotification(notifications.NotificationOptions{ID: id, Title: title, Body: body})
}

// enterLightweightMode saves residency and closes UI as one lifecycle transition.
// The settings watcher applies the saved mode after UI teardown; the idle host
// then exits without stopping the daemon.
func (h *HostService) enterLightweightMode() {
	h.tray.mu.Lock()
	message := backgroundNotices[h.tray.language]
	h.tray.mu.Unlock()
	h.ui.mu.Lock()
	defer h.ui.mu.Unlock()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	body := map[string]any{"general": map[string]any{"startupMode": startupLightweight}}
	if err := h.daemon().Enveloped(ctx, daemonclient.Request{Method: http.MethodPut, Path: "/settings", JSON: body}, nil); err != nil {
		log.Printf("failed to save lightweight mode: %v", err)
		h.app.Dialog.Error().SetTitle("UniClipboard").SetMessage(err.Error()).Show()
		return
	}
	h.ui.stopLocked()
	if err := h.notify("lightweight-"+strconv.FormatInt(time.Now().UnixNano(), 10), "UniClipboard", message); err != nil {
		log.Printf("failed to show the lightweight-mode notification: %v", err)
	}
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
