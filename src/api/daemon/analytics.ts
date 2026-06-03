/**
 * Analytics API module — `POST /analytics/capture` (ADR-008 D20).
 *
 * Analytics API 模块 — `POST /analytics/capture`（ADR-008 D20）。
 *
 * The daemon is the single authoritative product-analytics sender. The GUI
 * webview routes its UI-interaction events through this endpoint instead of an
 * in-process sink so the two processes never double-count device-level signals
 * in PostHog.
 *
 * daemon 是 product analytics 的唯一权威发送方。GUI webview 的 UI 交互事件经
 * 此端点上报，而非进程内 sink，避免两进程在 PostHog 重复计数设备级信号。
 */

import { captureUiEvent as captureUiEventSdk } from '@/api/generated/sdk.gen'
import type { CaptureUiEventRequest } from '@/api/generated/types.gen'
import { daemonClient } from './client'

export type { CaptureUiEventRequest }

/**
 * Capture a GUI UI-interaction event via the daemon.
 *
 * Fire-and-forget on the wire: the resolved value only confirms the daemon
 * decoded the event and handed it to its sink, not that it reached PostHog.
 *
 * @param event Discriminated-union event payload (by `kind`).
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function captureUiEvent(event: CaptureUiEventRequest): Promise<void> {
  await daemonClient.callSdk(() => captureUiEventSdk({ body: event, throwOnError: true }))
}
