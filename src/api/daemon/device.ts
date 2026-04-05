/**
 * Daemon device API module — typed HTTP client for per-device sync settings.
 *
 * Daemon 设备 API 模块 — 设备同步设置的类型化 HTTP 客户端。
 *
 * # Endpoints / 端点
 * - `GET /device/:peer_id/sync-settings` → resolved sync settings for a paired device
 * - `PATCH /device/:peer_id/sync-settings` → update per-device sync settings
 *
 * # Note / 注意
 * These replace the Tauri commands `get_device_sync_settings` and
 * `update_device_sync_settings` in `p2p.ts`.
 */

import { daemonClient } from './client'

// ── HTTP route constants ────────────────────────────────────────

const DEVICE_SYNC_SETTINGS = (peerId: string) => `/device/${peerId}/sync-settings`

// ── Sub-setting interfaces ─────────────────────────────────────

/** Content type toggles for sync filtering. */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

/** Sync frequency mode. */
export type SyncFrequency = 'realtime' | 'interval'

/**
 * Resolved sync settings for a paired device.
 * Matches `DeviceSyncSettingsDto` on the Rust side.
 */
export interface DeviceSyncSettings {
  autoSync: boolean
  syncFrequency: SyncFrequency
  contentTypes: ContentTypes
}

/**
 * Partial sync settings for PATCH.
 * Matches `DeviceSyncSettingsPatchDto` on the Rust side.
 */
export interface DeviceSyncSettingsPatch {
  autoSync?: boolean
  syncFrequency?: SyncFrequency
  contentTypes?: ContentTypesPatch
}

/** Partial content types for PATCH. */
export interface ContentTypesPatch {
  text?: boolean | null
  image?: boolean | null
  link?: boolean | null
  file?: boolean | null
  codeSnippet?: boolean | null
  richText?: boolean | null
}

// ── Response wrappers ──────────────────────────────────────────

/** GET /device/:peer_id/sync-settings response shape. */
interface DeviceSyncSettingsGetResponse {
  data: DeviceSyncSettings
  ts: number
}

/** PATCH /device/:peer_id/sync-settings response shape. */
interface DeviceSyncSettingsUpdateResponse {
  success: boolean
  data: DeviceSyncSettings
  ts: number
}

// ── Public API ─────────────────────────────────────────────────

/**
 * Fetch resolved sync settings for a paired device.
 *
 * 获取已配对设备的解析后同步设置。
 *
 * @param peerId The paired device's peer ID.
 * @returns The resolved sync settings (per-device overrides merged with global defaults).
 * @throws {DaemonApiError} On HTTP or session errors.
 */
export async function getDeviceSyncSettings(peerId: string): Promise<DeviceSyncSettings> {
  const res = await daemonClient.request<DeviceSyncSettingsGetResponse>(
    DEVICE_SYNC_SETTINGS(peerId)
  )
  return res.data
}

/**
 * Update per-device sync settings via partial merge on the server.
 *
 * 通过服务器端部分合并更新设备同步设置。
 *
 * Only the provided fields are changed; omitted fields retain their current values.
 * To reset all per-device overrides to global defaults, use `updateDeviceSyncSettings(peerId, null)`.
 *
 * @param peerId The paired device's peer ID.
 * @param patch Partial settings payload (or null to reset to global defaults).
 * @throws {DaemonApiError} On HTTP or validation errors.
 */
export async function updateDeviceSyncSettings(
  peerId: string,
  patch: DeviceSyncSettingsPatch | null
): Promise<DeviceSyncSettings> {
  if (patch === null) {
    // Reset: send a minimal patch that clears per-device overrides.
    // The server treats this as a reset by storing null / removing per-device settings.
    const res = await daemonClient.request<DeviceSyncSettingsUpdateResponse>(
      DEVICE_SYNC_SETTINGS(peerId),
      { method: 'PATCH', body: { autoSync: null } }
    )
    return res.data
  }

  const res = await daemonClient.request<DeviceSyncSettingsUpdateResponse>(
    DEVICE_SYNC_SETTINGS(peerId),
    { method: 'PATCH', body: patch }
  )
  return res.data
}
