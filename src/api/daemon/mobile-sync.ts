/**
 * Mobile-sync daemon API — typed accessors over the loopback HTTP endpoints
 * via the generated SDK (ADR-008 P3-b).
 *
 * # Endpoints / 端点
 * - `POST   /mobile-sync/devices`                         → register a device
 * - `GET    /mobile-sync/devices`                         → list devices
 * - `DELETE /mobile-sync/devices/{id}`                    → revoke a device
 * - `POST   /mobile-sync/devices/{id}/rotate-password`    → rotate password
 * - `GET    /mobile-sync/settings`                        → settings view
 * - `PATCH  /mobile-sync/settings`                        → update settings
 * - `GET    /mobile-sync/lan-interfaces`                  → LAN interfaces
 *
 * Thin transport layer: each call goes through `daemonClient.callEnveloped`,
 * which unwraps the daemon's canonical `{ data, ts }` envelope to the payload
 * and normalizes a thrown error into a `DaemonApiError` whose `.details`
 * carries the `{ code, message, details? }` body. The semantic-error translation back
 * into the `MobileSyncError` union lives in the public wrapper
 * (`src/api/tauri-command/mobile_sync.ts`). The two QR PNGs arrive base64 from
 * the daemon, ready for `<img src="data:image/png;base64,...">`.
 */

import {
  getMobileSyncSettings as getMobileSyncSettingsSdk,
  listMobileDevices as listMobileDevicesSdk,
  listMobileLanInterfaces as listMobileLanInterfacesSdk,
  registerMobileDevice as registerMobileDeviceSdk,
  revokeMobileDevice as revokeMobileDeviceSdk,
  rotateMobilePassword as rotateMobilePasswordSdk,
  updateMobileSyncSettings as updateMobileSyncSettingsSdk,
} from '@/api/generated/sdk.gen'
import type {
  LanInterfaceViewDto,
  MobileDeviceViewDto,
  MobileSyncSettingsViewDto,
  RegisterMobileDeviceRequest,
  RegisterMobileDeviceResultDto,
  RotateMobilePasswordRequest,
  RotateMobilePasswordResultDto,
  UpdateMobileSyncSettingsRequest,
  UpdateMobileSyncSettingsResultDto,
} from '@/api/generated/types.gen'
import { daemonClient } from './client'

/** `POST /mobile-sync/devices` — returns the one-time credentials + QR PNGs. */
export async function registerMobileDevice(
  body: RegisterMobileDeviceRequest
): Promise<RegisterMobileDeviceResultDto> {
  return daemonClient.callEnveloped(() => registerMobileDeviceSdk({ body, throwOnError: true }))
}

/** `GET /mobile-sync/devices` — registered devices, sorted by recent activity. */
export async function listMobileDevices(): Promise<MobileDeviceViewDto[]> {
  return daemonClient.callEnveloped(() => listMobileDevicesSdk({ throwOnError: true }))
}

/** `DELETE /mobile-sync/devices/{id}` — revoke a device's credentials. */
export async function revokeMobileDevice(deviceId: string): Promise<void> {
  await daemonClient.callSdk(() =>
    revokeMobileDeviceSdk({ path: { device_id: deviceId }, throwOnError: true })
  )
}

/** `POST /mobile-sync/devices/{id}/rotate-password` — one-time new password echo. */
export async function rotateMobilePassword(
  deviceId: string,
  body: RotateMobilePasswordRequest
): Promise<RotateMobilePasswordResultDto> {
  return daemonClient.callEnveloped(() =>
    rotateMobilePasswordSdk({ path: { device_id: deviceId }, body, throwOnError: true })
  )
}

/** `GET /mobile-sync/settings` — settings + LAN URL parts + install methods. */
export async function getMobileSyncSettings(): Promise<MobileSyncSettingsViewDto> {
  return daemonClient.callEnveloped(() => getMobileSyncSettingsSdk({ throwOnError: true }))
}

/** `PATCH /mobile-sync/settings` — three-state patch (see request DTO docs). */
export async function updateMobileSyncSettings(
  body: UpdateMobileSyncSettingsRequest
): Promise<UpdateMobileSyncSettingsResultDto> {
  return daemonClient.callEnveloped(() => updateMobileSyncSettingsSdk({ body, throwOnError: true }))
}

/** `GET /mobile-sync/lan-interfaces` — RFC1918 IPv4 candidates for the QR URL. */
export async function listMobileLanInterfaces(): Promise<LanInterfaceViewDto[]> {
  return daemonClient.callEnveloped(() => listMobileLanInterfacesSdk({ throwOnError: true }))
}
