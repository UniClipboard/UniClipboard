/**
 * Mobile sync public API wrappers.
 *
 * ADR-008 P3-b: the GUI is a cross-process client, so these used to be
 * in-process Tauri commands and now ride the daemon loopback HTTP API through
 * the generated SDK (`src/api/daemon/mobile-sync.ts`). The path name is kept
 * (`tauri-command/`) so the ~6 consumer call sites are unchanged.
 *
 * This module owns the FE-facing contract that the specta bindings used to
 * generate: the `MobileSyncError` discriminated union (with its structured
 * fields), the request/result types (narrowed over the generated DTOs), and
 * `isMobileSyncError`. Errors thrown by the daemon wrappers arrive as
 * `DaemonApiError`; `toMobileSyncError` reconstructs the same `{ code,
 * ...fields }` union consumers already `switch (err.code)` on, off
 * `DaemonApiError.details` (`{ code, message, details? }`).
 *
 * Backend: `src-tauri/crates/uc-webserver/src/api/mobile_sync.rs`
 */

import { DaemonApiError } from '@/api/daemon/errors'
import * as daemon from '@/api/daemon/mobile-sync'
import type {
  LanInterfaceViewDto,
  MobileDeviceViewDto,
  RegisterMobileDeviceRequest,
  RegisterMobileDeviceResultDto,
  MobileSyncSettingsViewDto,
  ShortcutInstallMethodViewDto,
  UpdateMobileDeviceRequest,
  UpdateMobileDeviceResultDto,
  UpdateMobileSyncSettingsRequest,
  UpdateMobileSyncSettingsResultDto,
} from '@/api/generated/types.gen'

// ============================================================================
// Error taxonomy ─ hand-authored union (wire-identical to the old specta one)
// ============================================================================

/**
 * Typed mobile-sync error. Serialized form on the daemon side is
 * `{ "code": "USERNAME_TAKEN", "username": "..." }`; the structured fields are
 * what `AddMobileSyncDeviceDialog` reads for i18n interpolation. Reconstructed
 * by {@link toMobileSyncError} from `DaemonApiError.details`.
 */
export type MobileSyncError =
  | { code: 'FACADE_UNAVAILABLE' }
  | { code: 'LABEL_EMPTY' }
  | { code: 'LABEL_TOO_LONG'; max: number }
  | { code: 'LAN_LISTENER_DISABLED' }
  | { code: 'USERNAME_TAKEN'; username: string }
  | { code: 'USERNAME_TOO_SHORT'; min: number; got: number }
  | { code: 'USERNAME_TOO_LONG'; max: number; got: number }
  | { code: 'USERNAME_MUST_START_WITH_LETTER' }
  | { code: 'USERNAME_CONTAINS_FORBIDDEN_CHARS' }
  | { code: 'PASSWORD_TOO_SHORT'; min: number }
  | { code: 'PASSWORD_TOO_LONG'; max: number }
  | { code: 'PASSWORD_HASH_FAILED'; message: string }
  | { code: 'DEVICE_NOT_FOUND'; deviceId: string }
  | { code: 'INVALID_LAN_PARAMETER'; reason: string }
  | { code: 'SETTINGS_LOAD_FAILED'; message: string }
  | { code: 'SETTINGS_SAVE_FAILED'; message: string }
  | { code: 'ENDPOINT_INFO_FAILED'; message: string }
  | { code: 'LAN_PROBE_FAILED'; message: string }
  | { code: 'NO_LAN_INTERFACE_AVAILABLE' }
  | { code: 'PERSISTENCE_FAILED'; message: string }
  | { code: 'QR_RENDER_FAILED'; message: string }

/** Every semantic `code` the daemon emits for mobile-sync. */
const MOBILE_SYNC_CODES: ReadonlySet<string> = new Set([
  'FACADE_UNAVAILABLE',
  'LABEL_EMPTY',
  'LABEL_TOO_LONG',
  'LAN_LISTENER_DISABLED',
  'USERNAME_TAKEN',
  'USERNAME_TOO_SHORT',
  'USERNAME_TOO_LONG',
  'USERNAME_MUST_START_WITH_LETTER',
  'USERNAME_CONTAINS_FORBIDDEN_CHARS',
  'PASSWORD_TOO_SHORT',
  'PASSWORD_TOO_LONG',
  'PASSWORD_HASH_FAILED',
  'DEVICE_NOT_FOUND',
  'INVALID_LAN_PARAMETER',
  'SETTINGS_LOAD_FAILED',
  'SETTINGS_SAVE_FAILED',
  'ENDPOINT_INFO_FAILED',
  'LAN_PROBE_FAILED',
  'NO_LAN_INTERFACE_AVAILABLE',
  'PERSISTENCE_FAILED',
  'QR_RENDER_FAILED',
])

/**
 * Type guard for catch blocks: widens an unknown error back to the typed union
 * so call sites can `switch (err.code)`. (Daemon errors are translated to this
 * shape by {@link toMobileSyncError} before being thrown.)
 */
export function isMobileSyncError(error: unknown): error is MobileSyncError {
  return (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    typeof (error as { code: unknown }).code === 'string' &&
    MOBILE_SYNC_CODES.has((error as { code: string }).code)
  )
}

/**
 * Reconstruct a `MobileSyncError` from a thrown `DaemonApiError`. The semantic
 * `code` and structured fields live on `DaemonApiError.details` (the normalized
 * `{ code, message, details? }` body): `details.code` is the tag, `details.details`
 * carries the per-variant fields. A bare facade-unavailable 503 surfaces as
 * `runtime_unavailable` (from `app_facade_or_error`) and is folded into
 * `FACADE_UNAVAILABLE`. Non-daemon / unrecognized errors pass through unchanged
 * so they are not masked as typed mobile-sync errors.
 */
function toMobileSyncError(error: unknown): unknown {
  if (!(error instanceof DaemonApiError)) {
    return error
  }
  const body = error.details as { code?: string; details?: Record<string, unknown> } | undefined
  const rawCode = body?.code
  if (rawCode === 'runtime_unavailable') {
    return { code: 'FACADE_UNAVAILABLE' } as MobileSyncError
  }
  if (typeof rawCode === 'string' && MOBILE_SYNC_CODES.has(rawCode)) {
    return { code: rawCode, ...(body?.details ?? {}) } as MobileSyncError
  }
  return error
}

// ============================================================================
// DTO types — narrow re-exports of generated bindings
// ============================================================================

/** Register-device request. Wire shape = generated `RegisterMobileDeviceRequest`. */
export type RegisterMobileDeviceArgs = RegisterMobileDeviceRequest

/** Edit-device request. `password` absent keeps it; null auto-generates. */
export type UpdateMobileDeviceArgs = UpdateMobileDeviceRequest & {
  deviceId: string
}

/** Update-settings patch. Wire shape = generated `UpdateMobileSyncSettingsRequest`. */
export type UpdateMobileSyncSettingsArgs = UpdateMobileSyncSettingsRequest

export type UpdateMobileDeviceResult = UpdateMobileDeviceResultDto
export type UpdateMobileSyncSettingsResult = UpdateMobileSyncSettingsResultDto
export type LanInterfaceView = LanInterfaceViewDto

/**
 * Mobile client kind. The daemon serializes a `String`; the only value the
 * server currently mints is `'ios_shortcut'`. The narrower TS type lets callers
 * `switch (clientType)` exhaustively.
 */
export type MobileClientType = 'ios_shortcut'

/** Shortcut install delivery method — same justification as `MobileClientType`. */
export type ShortcutInstallMethodKind = 'tokenInjected' | 'icloudGeneric'

/** `MobileDeviceViewDto` with narrowed `clientType` literal type. */
export type MobileDeviceView = Omit<MobileDeviceViewDto, 'clientType'> & {
  clientType: MobileClientType
}

/** Same narrowing as `MobileDeviceView`. */
export type RegisterMobileDeviceResult = Omit<RegisterMobileDeviceResultDto, 'clientType'> & {
  clientType: MobileClientType
}

/** Same narrowing on `method` field. */
export type ShortcutInstallMethodView = Omit<ShortcutInstallMethodViewDto, 'method'> & {
  method: ShortcutInstallMethodKind
}

/**
 * Settings view aliased back to the historical short name for ergonomic
 * imports; the wire shape is exactly `MobileSyncSettingsViewDto`, with
 * `shortcutInstallMethods` narrowed.
 */
export type MobileSyncSettingsView = Omit<MobileSyncSettingsViewDto, 'shortcutInstallMethods'> & {
  shortcutInstallMethods: ShortcutInstallMethodView[]
}

// ============================================================================
// Command wrappers (loopback via generated SDK, typed-error translation)
// ============================================================================

/**
 * Register a new iPhone Shortcut device. Returns the long-lived credentials
 * along with the install QR code. The plaintext password in the response is
 * shown to the user only once — see `RegisterMobileDeviceResult.password`.
 *
 * @throws {MobileSyncError} typed union — `switch` on `error.code`.
 */
export async function registerMobileDevice(
  args: RegisterMobileDeviceArgs
): Promise<RegisterMobileDeviceResult> {
  try {
    const result = await daemon.registerMobileDevice(args)
    return result as RegisterMobileDeviceResult
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * Revoke a previously registered device. After this returns, the device's
 * (username, password) pair is invalid; the iPhone will get 401 on next
 * request.
 */
export async function revokeMobileDevice(deviceId: string): Promise<void> {
  try {
    await daemon.revokeMobileDevice(deviceId)
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * Edit a registered mobile device. A label-only edit returns no plaintext
 * password; username/password edits return a one-time password echo.
 */
export async function updateMobileDevice(
  args: UpdateMobileDeviceArgs
): Promise<UpdateMobileDeviceResult> {
  try {
    return await daemon.updateMobileDevice(args.deviceId, {
      label: args.label,
      username: args.username,
      password: args.password,
    })
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * List currently registered devices, sorted by recent activity then
 * registration time. Does NOT include `password_hash`. `username` is
 * exposed as an auxiliary identifier for the UI (paired with `label`).
 */
export async function listMobileDevices(): Promise<MobileDeviceView[]> {
  try {
    const devices = await daemon.listMobileDevices()
    return devices as MobileDeviceView[]
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * Read the current mobile sync settings, the live LAN URL, and the
 * available shortcut install methods.
 */
export async function getMobileSyncSettings(): Promise<MobileSyncSettingsView> {
  try {
    const view = await daemon.getMobileSyncSettings()
    return view as MobileSyncSettingsView
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * Update the persisted mobile sync settings. Pass only the fields you
 * intend to change (see `UpdateMobileSyncSettingsArgs` for three-state
 * semantics on nullable fields).
 */
export async function updateMobileSyncSettings(
  args: UpdateMobileSyncSettingsArgs
): Promise<UpdateMobileSyncSettingsResult> {
  try {
    return await daemon.updateMobileSyncSettings(args)
  } catch (error) {
    throw toMobileSyncError(error)
  }
}

/**
 * List candidate IPv4 LAN interfaces for the QR code URL. Only RFC1918
 * private addresses, sorted 10/8 → 172.16/12 → 192.168/16.
 */
export async function listMobileLanInterfaces(): Promise<LanInterfaceView[]> {
  try {
    return await daemon.listMobileLanInterfaces()
  } catch (error) {
    throw toMobileSyncError(error)
  }
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
