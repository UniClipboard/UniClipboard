/**
 * Mobile sync Tauri command wrappers.
 *
 * GUI 走 in-process facade 直调 `uc-application::MobileSyncFacade`，不经过
 * daemon webserver（参见 memory `project_gui_uses_inprocess_facade.md`）。
 *
 * 本文件作为 *thin re-export*：契约真正的真相源是 `src/lib/ipc.ts` 的
 * 类型化 commands proxy（背后是 `cargo test --test specta_export` 生成的
 * `ipc-bindings.generated.ts`）。这里只做两件事：
 * 1. 把 `commands.xxx` 重命名成历史调用方习惯的函数名；
 * 2. 把生成的 DTO 类型重导出，并保留少量更窄的 TS-only 联合类型
 *    （如 `MobileClientType = 'ios_shortcut'`），让调用方拿到比 Rust
 *    `String` 更精确的类型信息。
 *
 * Backend: `src-tauri/crates/uc-tauri/src/commands/mobile_sync.rs`
 */

import { commands } from '@/lib/ipc'
import type {
  LanInterfaceView,
  MobileDeviceView as GeneratedMobileDeviceView,
  MobileSyncError,
  MobileSyncSettingsViewDto,
  RegisterMobileDeviceArgs,
  RegisterMobileDeviceResult as GeneratedRegisterMobileDeviceResult,
  RotateMobilePasswordArgs,
  RotateMobilePasswordResult,
  ShortcutInstallMethodView as GeneratedShortcutInstallMethodView,
  UpdateMobileSyncSettingsArgs,
  UpdateMobileSyncSettingsResult,
} from '@/lib/ipc'

// ============================================================================
// Re-exported error taxonomy + type guard
// ============================================================================

export type { MobileSyncError }

/**
 * Type guard for catch blocks. Tauri rejects with the typed-error envelope
 * shape (`{ code: '...' , ...payload }`); this widens unknown errors back
 * to the typed union so call sites can do `switch (err.code)`.
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
// DTO types — narrow re-exports of generated bindings
// ============================================================================

/**
 * Mobile client kind. Rust serializes a `String` here; the only value the
 * server currently mints is `'ios_shortcut'`. The narrower TS type lets
 * callers `switch (clientType)` exhaustively.
 */
export type MobileClientType = 'ios_shortcut'

/** Shortcut install delivery method —— same justification as `MobileClientType`. */
export type ShortcutInstallMethodKind = 'tokenInjected' | 'icloudGeneric'

export type {
  RegisterMobileDeviceArgs,
  RotateMobilePasswordArgs,
  RotateMobilePasswordResult,
  UpdateMobileSyncSettingsArgs,
  UpdateMobileSyncSettingsResult,
  LanInterfaceView,
}

/** `MobileDeviceView` with narrowed `clientType` literal type. */
export type MobileDeviceView = Omit<GeneratedMobileDeviceView, 'clientType'> & {
  clientType: MobileClientType
}

/** Same narrowing as `MobileDeviceView`. */
export type RegisterMobileDeviceResult = Omit<GeneratedRegisterMobileDeviceResult, 'clientType'> & {
  clientType: MobileClientType
}

/** Same narrowing on `method` field. */
export type ShortcutInstallMethodView = Omit<GeneratedShortcutInstallMethodView, 'method'> & {
  method: ShortcutInstallMethodKind
}

/**
 * Settings view aliased back to the historical short name for ergonomic
 * imports; the wire shape is exactly `MobileSyncSettingsViewDto` from the
 * generated bindings, with `shortcutInstallMethods` narrowed.
 */
export type MobileSyncSettingsView = Omit<MobileSyncSettingsViewDto, 'shortcutInstallMethods'> & {
  shortcutInstallMethods: ShortcutInstallMethodView[]
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
  const result = await commands.registerMobileDevice(args)
  return result as RegisterMobileDeviceResult
}

/**
 * Revoke a previously registered device. After this returns, the device's
 * (username, password) pair is invalid; the iPhone will get 401 on next
 * request.
 */
export async function revokeMobileDevice(deviceId: string): Promise<void> {
  await commands.revokeMobileDevice(deviceId)
}

/**
 * Rotate the password of a previously registered device. Old password is
 * invalidated atomically on the daemon side; the response carries the
 * one-time plaintext of the new password — see `RotateMobilePasswordResult`.
 */
export async function rotateMobilePassword(
  args: RotateMobilePasswordArgs
): Promise<RotateMobilePasswordResult> {
  return await commands.rotateMobilePassword(args)
}

/**
 * List currently registered devices, sorted by recent activity then
 * registration time. Does NOT include `password_hash`. `username` is
 * exposed as an auxiliary identifier for the UI (paired with `label`).
 */
export async function listMobileDevices(): Promise<MobileDeviceView[]> {
  const devices = await commands.listMobileDevices()
  return devices as MobileDeviceView[]
}

/**
 * Read the current mobile sync settings, the live LAN URL, and the
 * available shortcut install methods.
 */
export async function getMobileSyncSettings(): Promise<MobileSyncSettingsView> {
  const view = await commands.getMobileSyncSettings()
  return view as MobileSyncSettingsView
}

/**
 * Update the persisted mobile sync settings. Pass only the fields you
 * intend to change (see `UpdateMobileSyncSettingsArgs` for three-state
 * semantics on nullable fields).
 */
export async function updateMobileSyncSettings(
  args: UpdateMobileSyncSettingsArgs
): Promise<UpdateMobileSyncSettingsResult> {
  return await commands.updateMobileSyncSettings(args)
}

/**
 * List candidate IPv4 LAN interfaces for the QR code URL. Only RFC1918
 * private addresses, sorted 10/8 → 172.16/12 → 192.168/16.
 */
export async function listMobileLanInterfaces(): Promise<LanInterfaceView[]> {
  return await commands.listMobileLanInterfaces()
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
