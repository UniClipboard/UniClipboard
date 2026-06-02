/**
 * Daemon member API module — typed HTTP client for per-member sync preferences.
 *
 * Phase 4b PR-3：取代 `./device.ts` 里读写 `PairedDevice.sync_settings` 的旧路径,
 * 对应后端 `MemberSyncPreferencesDto` (双向 send/receive + 双套 content types)。
 *
 * # Endpoints / 端点
 * - `GET /member/:device_id/sync-preferences`
 * - `PATCH /member/:device_id/sync-preferences`
 *
 * 本阶段前端 UX 只展示 send 列；receive 字段不露出,由后端默认值保持
 * (`MemberSyncPreferences::default()`: `send/receive = true`,两套 `ContentTypes` 全开)。
 * 未来若要暴露接收开关,直接扩展本模块与 `DeviceSettingsSheet` 即可。
 */

import { daemonClient } from './client'

// ── Route constant ──────────────────────────────────────────────

const MEMBER_SYNC_PREFERENCES = (deviceId: string) => `/member/${deviceId}/sync-preferences`

// ── Value objects ───────────────────────────────────────────────

/** Content type toggles. Matches `ContentTypesDto` on the Rust side. */
export interface ContentTypes {
  text: boolean
  image: boolean
  link: boolean
  file: boolean
  codeSnippet: boolean
  richText: boolean
}

/** Partial content type toggles for PATCH. */
export interface ContentTypesPatch {
  text?: boolean
  image?: boolean
  link?: boolean
  file?: boolean
  codeSnippet?: boolean
  richText?: boolean
}

/**
 * Sync preferences recorded for a space member.
 * Matches `MemberSyncPreferencesDto` on the Rust side.
 */
export interface MemberSyncPreferences {
  sendEnabled: boolean
  receiveEnabled: boolean
  sendContentTypes: ContentTypes
  receiveContentTypes: ContentTypes
}

/**
 * Partial member sync preferences for PATCH.
 * Any omitted top-level field keeps its current value server-side.
 */
export interface MemberSyncPreferencesPatch {
  sendEnabled?: boolean
  receiveEnabled?: boolean
  sendContentTypes?: ContentTypesPatch
  receiveContentTypes?: ContentTypesPatch
}

// ── Response wrappers ───────────────────────────────────────────

interface MemberSyncPreferencesGetResponse {
  data: MemberSyncPreferences
  ts: number
}

/**
 * PATCH /member/:id/sync-preferences response (ADR-008): the daemon now returns
 * only `{ data: { success }, ts }` (MemberSyncResultEnvelope) and no longer echoes
 * the full preferences, so callers re-fetch the authoritative merged value via GET.
 */
interface MemberSyncResultResponse {
  data: { success: boolean }
  ts: number
}

// ── Public API ──────────────────────────────────────────────────

export async function getMemberSyncPreferences(deviceId: string): Promise<MemberSyncPreferences> {
  const res = await daemonClient.request<MemberSyncPreferencesGetResponse>(
    MEMBER_SYNC_PREFERENCES(deviceId)
  )
  return res.data
}

export async function updateMemberSyncPreferences(
  deviceId: string,
  patch: MemberSyncPreferencesPatch
): Promise<MemberSyncPreferences> {
  // PATCH returns only `{ data: { success } }` (ApiEnvelope<MemberSyncResultDto>);
  // request() throws on non-2xx, so reaching here means success. Re-fetch to return
  // the authoritative merged preferences (preserves the Promise<MemberSyncPreferences>
  // contract that devicesSlice stores into state — the PATCH body no longer echoes it).
  await daemonClient.request<MemberSyncResultResponse>(MEMBER_SYNC_PREFERENCES(deviceId), {
    method: 'PATCH',
    body: patch,
  })
  return getMemberSyncPreferences(deviceId)
}

// ── Default-value helpers (used by "restore defaults" button) ──

/**
 * Mirror of `MemberSyncPreferences::default()` on the Rust side (all toggles on).
 * Used by Restore Defaults to push an explicit reset without relying on a
 * server-side "null clears overrides" semantic (which does not exist for
 * `space_member`; every member always has a preferences record).
 */
export const DEFAULT_SEND_CONTENT_TYPES: ContentTypes = {
  text: true,
  image: true,
  link: true,
  file: true,
  codeSnippet: true,
  richText: true,
}
