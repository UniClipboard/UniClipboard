/**
 * Mobile sync Tauri command wrappers.
 *
 * GUI 走 in-process facade 直调 `uc-application::MobileSyncFacade`，
 * 不经过 daemon webserver。这是 GUI in-process facade 模式的第一例；
 * 未来 webserver 会逐步从 GUI 路径中移除（参见
 * `.claude/projects/.../memory/project_gui_uses_inprocess_facade.md`）。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs`
 */

import { invokeWithTrace } from '@/lib/tauri-command'

// ============================================================================
// Error taxonomy
// ============================================================================

/**
 * Discriminated union mirroring `MobileSyncError` on the Rust side.
 *
 * Frontend pattern matches on `code` to render localized messages and
 * branch on rich payload fields (e.g., `min` for password length).
 */
export type MobileSyncError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'LABEL_EMPTY' }
  | { code: 'LABEL_TOO_LONG'; max: number }
  | { code: 'LAN_LISTENER_DISABLED' }
  | { code: 'USERNAME_TAKEN'; username: string }
  | { code: 'USERNAME_INVALID_SHAPE'; reason: string }
  | { code: 'PASSWORD_TOO_SHORT'; min: number }
  | { code: 'PASSWORD_TOO_LONG'; max: number }
  | { code: 'PASSWORD_HASH_FAILED'; message: string }
  | { code: 'DEVICE_NOT_FOUND'; deviceId: string }
  | { code: 'INVALID_LAN_PARAMETER'; reason: string }
  | { code: 'SETTINGS_LOAD_FAILED'; message: string }
  | { code: 'SETTINGS_SAVE_FAILED'; message: string }
  | { code: 'ENDPOINT_INFO_PROBE_FAILED'; message: string }
  | { code: 'LAN_PROBE_FAILED'; message: string }
  | { code: 'NO_LAN_INTERFACE_AVAILABLE' }
  | { code: 'PERSISTENCE_FAILED'; message: string }
  | { code: 'QR_RENDER_FAILED'; message: string }

/**
 * Type guard for catch blocks. Tauri rejects with a plain object that
 * matches the JSON shape; this widens it back to a typed union.
 */
export function isMobileSyncError(error: unknown): error is MobileSyncError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string'
  )
}

// ============================================================================
// DTO types — mirror Rust serde shape (camelCase fields)
// ============================================================================

export type MobileClientType = 'ios_shortcut'

export type ShortcutInstallMethodKind = 'tokenInjected' | 'icloudGeneric'

export interface RegisterMobileDeviceArgs {
  label: string
  /** Optional custom username; leave undefined to auto-mint. */
  username?: string
  /** Optional custom password; leave undefined to auto-mint. */
  password?: string
}

export interface RegisterMobileDeviceResult {
  deviceId: string
  label: string
  clientType: MobileClientType
  createdAtMs: number
  baseUrl: string
  username: string
  /**
   * Plaintext password — returned **only this once**.
   *
   * After the modal that surfaces this value closes, the password is
   * unrecoverable (server only stores the Argon2id PHC hash). UI must:
   * 1. Show prominently in a copy-friendly modal
   * 2. Block close until the user explicitly confirms they have saved it
   * 3. Never log, persist, or send to any analytics
   */
  password: string
  /** SyncClipboard "Clipboard EX" iCloud share URL (constant). */
  installUrl: string
  /** Base64-encoded PNG; render via `<img src="data:image/png;base64,..." />`. */
  qrCodePngBase64: string
}

export interface MobileDeviceView {
  deviceId: string
  label: string
  clientType: MobileClientType
  createdAtMs: number
  lastSeenAtMs: number | null
  lastSeenIp: string | null
  reportedName: string | null
  reportedOs: string | null
}

export interface ShortcutInstallMethodView {
  method: ShortcutInstallMethodKind
  available: boolean
  /** Human-readable reason when `available === false`; `null` otherwise. */
  disabledReason: string | null
}

export interface MobileSyncSettingsView {
  /** Master switch for the entire mobile sync feature. */
  enabled: boolean
  /** Sub-switch for the LAN HTTP listener. */
  lanListenEnabled: boolean
  /** Persisted advertised IP; `null` means "0.0.0.0 / wildcard". */
  lanAdvertiseIp: string | null
  /** Persisted port; `null` falls back to 42720 at runtime. */
  lanPort: number | null
  /**
   * Daemon-side LAN listener bind failure reason (port in use / IP not
   * assignable / permission denied). `Some` means daemon actually attempted
   * bind and failed; `null` means "not started" or "bind succeeded".
   *
   * The display URL is derived purely from `lanAdvertiseIp` + `lanPort`
   * (daemon always binds `0.0.0.0:<lan_port>`). No runtime URL probe.
   */
  lanListenerError: string | null
  shortcutInstallMethods: ShortcutInstallMethodView[]
}

/**
 * Patch input for `update_mobile_sync_settings`.
 *
 * Three-state convention for `lanAdvertiseIp` / `lanPort`:
 * - field absent (or explicit `undefined` — JSON.stringify drops it):
 *   do not change
 * - explicit `null`: clear the persisted value
 * - concrete value: write it
 */
export interface UpdateMobileSyncSettingsArgs {
  enabled?: boolean
  lanListenEnabled?: boolean
  lanAdvertiseIp?: string | null
  lanPort?: number | null
}

export interface UpdateMobileSyncSettingsResult {
  enabled: boolean
  lanListenEnabled: boolean
  lanAdvertiseIp: string | null
  lanPort: number | null
  /** True iff any field actually changed; same-value saves are no-ops. */
  restartRequired: boolean
}

export interface LanInterfaceView {
  /** Interface name (`en0` / `eth0` / `Wi-Fi`) for disambiguation. */
  name: string
  /** Human-readable IPv4 string (`192.168.1.5`). */
  ipv4: string
}

// ============================================================================
// Command wrappers
// ============================================================================

/**
 * Register a new iPhone Shortcut device. Returns the long-lived credentials
 * along with the install QR code. The plaintext password in the response is
 * shown to the user only once — see `RegisterMobileDeviceResult.password`.
 */
export async function registerMobileDevice(
  args: RegisterMobileDeviceArgs
): Promise<RegisterMobileDeviceResult> {
  return await invokeWithTrace<RegisterMobileDeviceResult>('register_mobile_device', { args })
}

/**
 * Revoke a previously registered device. After this returns, the device's
 * (username, password) pair is invalid; the iPhone will get 401 on next
 * request.
 */
export async function revokeMobileDevice(deviceId: string): Promise<void> {
  await invokeWithTrace<void>('revoke_mobile_device', { deviceId })
}

/**
 * List currently registered devices, sorted by recent activity then
 * registration time. Does NOT include `password_hash` or `username`.
 */
export async function listMobileDevices(): Promise<MobileDeviceView[]> {
  return await invokeWithTrace<MobileDeviceView[]>('list_mobile_devices')
}

/**
 * Read the current mobile sync settings, the live LAN URL, and the
 * available shortcut install methods.
 */
export async function getMobileSyncSettings(): Promise<MobileSyncSettingsView> {
  return await invokeWithTrace<MobileSyncSettingsView>('get_mobile_sync_settings')
}

/**
 * Update the persisted mobile sync settings. Pass only the fields you
 * intend to change (see `UpdateMobileSyncSettingsArgs` for three-state
 * semantics on nullable fields).
 */
export async function updateMobileSyncSettings(
  args: UpdateMobileSyncSettingsArgs
): Promise<UpdateMobileSyncSettingsResult> {
  return await invokeWithTrace<UpdateMobileSyncSettingsResult>('update_mobile_sync_settings', {
    args,
  })
}

/**
 * List candidate IPv4 LAN interfaces for the QR code URL. Only RFC1918
 * private addresses, sorted 10/8 → 172.16/12 → 192.168/16.
 */
export async function listMobileLanInterfaces(): Promise<LanInterfaceView[]> {
  return await invokeWithTrace<LanInterfaceView[]>('list_mobile_lan_interfaces')
}

/**
 * Derive the user-facing listen URL from persisted settings. Daemon always
 * binds `0.0.0.0:<lan_port>`; the advertised host is whatever the user picked
 * (or `0.0.0.0` when unset). No runtime probe — pure projection from settings.
 */
export function deriveListenUrl(view: MobileSyncSettingsView): string {
  const host = view.lanAdvertiseIp ?? '0.0.0.0'
  const port = view.lanPort ?? 42720
  return `http://${host}:${port}`
}
