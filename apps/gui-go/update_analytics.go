package main

import (
	"context"
	"errors"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/UniClipboard/UniClipboard/apps/gui-go/internal/update"
	"github.com/UniClipboard/UniClipboard/packages/desktop-host-go/daemonclient"
)

// The update lifecycle events the native shell owns, forwarded to the daemon's `POST /analytics/capture` (the
// daemon is the single authoritative sender and applies the usage-analytics consent gate; the GUI never talks
// to an analytics backend). The shared React code already reports dialog_opened, dismissed and the UI-side
// action_invoked; the events below happen without a WebView and are reported here with the same wire shape as
// the Tauri shell (crates/uc-tauri: update_scheduler and commands/updater).
const (
	checkSourceManual    = "manual"
	checkSourceScheduled = "scheduled"

	// Wails creates the updater window without reporting a failure, so there is no send_failed or
	// permission_denied delivery to report: opening it is always "sent".
	deliverySent = "sent"

	analyticsTimeout = 5 * time.Second
)

// analyticsQueue serialises the sends so a pair such as started → succeeded reaches the daemon in order.
type analyticsQueue struct {
	once   sync.Once
	events chan map[string]any
}

const analyticsQueueSize = 64

// captureUpdateEvent is fire-and-forget: a failure is logged by event kind only (never the body) and never
// affects the update flow, like the Tauri shell. A full queue (daemon unreachable for a long time) drops events.
func (h *HostService) captureUpdateEvent(event map[string]any) {
	q := &h.analytics
	q.once.Do(func() {
		q.events = make(chan map[string]any, analyticsQueueSize)
		go func() {
			for event := range q.events {
				ctx, cancel := context.WithTimeout(context.Background(), analyticsTimeout)
				began := time.Now()
				err := h.client.Enveloped(ctx, daemonclient.Request{Method: http.MethodPost, Path: "/analytics/capture", JSON: event}, nil)
				cancel()
				if err != nil {
					log.Printf("update analytics: capture %v failed after %s: %v", event["kind"], time.Since(began).Round(time.Millisecond), err)
				}
			}
		}()
	})
	select {
	case q.events <- event:
	default:
		log.Printf("update analytics: queue full, dropped %v", event["kind"])
	}
}

// reportCheck sends update_check_performed for one finished check. failure_kind is present only for a failed
// check, and install_kind always, as in the Tauri shell.
func (h *HostService) reportCheck(source string, found bool, err error) {
	event := map[string]any{"kind": "check_performed", "source": source, "install_kind": installKind()}
	switch {
	case err != nil:
		event["outcome"], event["failure_kind"] = "failed", string(update.Classify(err))
	case found:
		event["outcome"] = "available"
	default:
		event["outcome"] = "up_to_date"
	}
	h.captureUpdateEvent(event)
}

// reportNotification sends update_notification_shown after the updater window was (or failed to be) opened.
func (h *HostService) reportNotification(version, delivery string) {
	h.captureUpdateEvent(map[string]any{"kind": "notification_shown", "version": version, "delivery_status": delivery, "install_kind": installKind()})
}

// downloadFailure classifies a download for update_action_invoked: a precondition rejection never started a
// download (no event), a cancel has its own outcome slot, anything else is a failed download.
type downloadFailure int

const (
	downloadOK downloadFailure = iota
	downloadPrecondition
	downloadCancelled
	downloadFailed
)

// preconditionError marks a download refused before it began and cancelledError one the user cancelled; both
// read as plain strings, but only commands convert them (`stringError(err.Error())`) before they cross the wire.
type preconditionError struct{ stringError }
type cancelledError struct{ stringError }

func classifyDownload(err error) downloadFailure {
	var pre preconditionError
	var cancelled cancelledError
	switch {
	case err == nil:
		return downloadOK
	case errors.As(err, &pre):
		return downloadPrecondition
	case errors.As(err, &cancelled):
		return downloadCancelled
	}
	return downloadFailed
}

// reportDownload sends the download_bg pair for a finished download: started once, then the terminal outcome.
// A refused download reports nothing so the funnel denominator stays clean.
func (h *HostService) reportDownload(err error) {
	kind := classifyDownload(err)
	if kind == downloadPrecondition {
		return
	}
	action := func(outcome string, errorKind string) {
		event := map[string]any{"kind": "action_invoked", "action": "download_bg", "outcome": outcome}
		if errorKind != "" {
			event["error_kind"] = errorKind
		}
		h.captureUpdateEvent(event)
	}
	action("started", "")
	switch kind {
	case downloadOK:
		action("succeeded", "")
	case downloadCancelled:
		action("cancelled", "")
	default:
		action("failed", "download_failed")
	}
}
